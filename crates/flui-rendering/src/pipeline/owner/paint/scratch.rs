//! The collections a paint pass fills and empties, and how much room the owner keeps in them
//! between passes.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_layer::{BoundaryContent, Effect, EntryIndex, SceneNodeId};
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use crate::context::FragmentRecorder;

use super::{
    ChildPaintOrder,
    composer::BoundaryBuild,
    kept_chunks::ChunkIndex,
    records::{Placed, Refreshed},
};

/// The collections a paint pass fills and empties, kept by the owner across
/// frames so a pass allocates only when it outgrows them.
///
/// A pass takes them out of the owner and puts them back once it committed;
/// one that fails or unwinds leaves empty ones behind, which serve the next
/// pass the same. Each is cleared when it is taken, so what a failed pass
/// left in one is never read.
#[derive(Debug, Default)]
pub(in crate::pipeline::owner) struct PaintScratch {
    /// The queued nodes, as a set.
    pub(super) dirty_ids: FxHashSet<RenderId>,
    /// The boundaries queued only for composited-layer updates.
    pub(super) layer_updates: FxHashMap<RenderId, SmallVec<[RenderId; 2]>>,
    pub(super) composer: ComposerScratch,
}

impl PaintScratch {
    /// Empties every collection for the next pass, dropping one this pass
    /// grew past [`KEPT_CAPACITY`]: clearing a hash table costs its capacity,
    /// so a large frame must not make every later small one pay for it.
    pub(super) fn recycle(&mut self) {
        recycle_set(&mut self.dirty_ids);
        recycle_map(&mut self.layer_updates);
        let composer = &mut self.composer;
        recycle_vec(&mut composer.builds);
        recycle_vec(&mut composer.publishes);
        recycle_vec(&mut composer.places);
        recycle_set(&mut composer.served);
        recycle_set(&mut composer.slotted);
        recycle_vec(&mut composer.child_paint_orders);
        recycle_vec(&mut composer.consumed_updates);
        recycle_set(&mut composer.visited);
        recycle_map(&mut composer.ordinals);
        recycle_map(&mut composer.effect_owner);
        recycle_vec(&mut composer.layer_patches);
        recycle_set(&mut composer.republished);
        recycle_vec(&mut composer.retired);
        recycle_set(&mut composer.retired_set);
        recycle_vec(&mut composer.unslotted);
        recycle_map(&mut composer.records);
        recycle_map(&mut composer.placed);
        recycle_vec(&mut composer.stale);
        recycle_vec(&mut composer.orphans);
        recycle_vec(&mut composer.own_effects);
        recycle_vec(&mut composer.painted_slots);
        composer.chunk_index.recycle(KEPT_CAPACITY);
        let peak = std::mem::take(&mut composer.fragment_peak);
        let kept = composer.fragment_room.after_pass(peak);
        let ops = composer.recorder.0.ops_mut();
        ops.clear();
        keep_room(ops, kept);
    }

    /// Hands back the maps a staged patch held, for the next pass to stage
    /// into.
    pub(super) fn recycle_staged(
        &mut self,
        mut records: FxHashMap<RenderId, Option<Refreshed>>,
        mut placed: FxHashMap<RenderId, Placed>,
        mut stale: Vec<RenderId>,
        mut orphans: Vec<SceneNodeId>,
    ) {
        recycle_map(&mut records);
        recycle_map(&mut placed);
        recycle_vec(&mut stale);
        recycle_vec(&mut orphans);
        let composer = &mut self.composer;
        composer.records = records;
        composer.placed = placed;
        composer.stale = stale;
        composer.orphans = orphans;
    }
}

/// The most entries a collection [`PaintScratch`] keeps between passes may
/// have room for.
const KEPT_CAPACITY: usize = 1024;

/// The least room for fragment operations the pass's shared buffer keeps
/// between passes, whatever the last passes needed: enough for the path down
/// a deep tree. Past it the buffer keeps what [`FragmentRoom`] answers.
const KEPT_FRAGMENT_OPS: usize = 64;

/// The passes [`FragmentRoom`] remembers.
const FRAGMENT_PASSES: usize = 8;

/// Trims the empty buffer `ops` to keep room for `kept` operations: none past
/// [`KEPT_FRAGMENT_OPS`] when `kept` is within it, and otherwise what it has
/// unless half of that would sit idle.
///
/// The slack lets a pass that needs a little more than the last find room and
/// one that needs a little less give none back, so a buffer grown by doubling
/// to the room it keeps is not cut and regrown each pass. A buffer therefore
/// holds at most twice [`KEPT_CAPACITY`] between passes, and a pass that
/// needs more than that regrows it.
fn keep_room<T>(ops: &mut Vec<T>, kept: usize) {
    debug_assert!(ops.is_empty(), "BUG: the room is kept for an empty buffer");
    if kept <= KEPT_FRAGMENT_OPS {
        if ops.capacity() > KEPT_FRAGMENT_OPS {
            *ops = Vec::new();
        }
    } else if ops.capacity() > kept.saturating_mul(2) {
        ops.shrink_to(kept);
    }
}

/// How much room the pass's shared fragment buffer keeps between passes: the
/// most that two of the last [`FRAGMENT_PASSES`] passes each needed, up to
/// [`KEPT_CAPACITY`] operations, which [`keep_room`] applies.
///
/// A size that recurs is kept, so a list scrolled frame after frame (up to
/// twice the cap) grows nothing even when a small repaint (a caret, a
/// scrollbar's fade) falls between its frames, while a size one pass needed
/// once, a mount, leaves no room behind.
#[derive(Debug, Default)]
struct FragmentRoom {
    /// The peaks of the last passes, the oldest overwritten first.
    peaks: [usize; FRAGMENT_PASSES],
    /// Where the next pass's peak goes.
    next: usize,
}

impl FragmentRoom {
    /// Records the peak of the pass that just ended and answers the room to
    /// keep for the next.
    fn after_pass(&mut self, peak: usize) -> usize {
        self.peaks[self.next] = peak;
        self.next = (self.next + 1) % FRAGMENT_PASSES;
        let (mut largest, mut second) = (0, 0);
        for &peak in &self.peaks {
            if peak > largest {
                second = largest;
                largest = peak;
            } else if peak > second {
                second = peak;
            }
        }
        second.min(KEPT_CAPACITY)
    }
}

fn recycle_map<K, V>(map: &mut FxHashMap<K, V>) {
    if map.capacity() > KEPT_CAPACITY {
        *map = FxHashMap::default();
    } else {
        map.clear();
    }
}

fn recycle_set<K>(set: &mut FxHashSet<K>) {
    if set.capacity() > KEPT_CAPACITY {
        *set = FxHashSet::default();
    } else {
        set.clear();
    }
}

fn recycle_vec<T>(vec: &mut Vec<T>) {
    if vec.capacity() > KEPT_CAPACITY {
        *vec = Vec::new();
    } else {
        vec.clear();
    }
}

/// What [`FragmentComposer`] works in, between passes.
#[derive(Debug, Default)]
pub(super) struct ComposerScratch {
    pub(super) builds: Vec<BoundaryBuild>,
    pub(super) publishes: Vec<(SceneNodeId, Arc<BoundaryContent>)>,
    pub(super) places: Vec<(SceneNodeId, Offset)>,
    pub(super) served: FxHashSet<RenderId>,
    pub(super) slotted: FxHashSet<SceneNodeId>,
    pub(super) child_paint_orders: Vec<(RenderId, Option<ChildPaintOrder>)>,
    pub(super) consumed_updates: Vec<RenderId>,
    pub(super) visited: FxHashSet<RenderId>,
    pub(super) ordinals: FxHashMap<(u64, RenderId), (u32, u32)>,
    pub(super) effect_owner: FxHashMap<(u64, RenderId), usize>,
    pub(super) layer_patches: Vec<(EntryIndex, Effect)>,
    pub(super) republished: FxHashSet<SceneNodeId>,
    pub(super) retired: Vec<SceneNodeId>,
    pub(super) retired_set: FxHashSet<SceneNodeId>,
    pub(super) unslotted: Vec<RenderId>,
    /// A staged patch's maps, handed back by `scene_applied`.
    pub(super) records: FxHashMap<RenderId, Option<Refreshed>>,
    pub(super) placed: FxHashMap<RenderId, Placed>,
    pub(super) stale: Vec<RenderId>,
    /// The orphan list the owner held before the last applied patch
    /// replaced it, emptied, for the next pass to collect into.
    pub(super) orphans: Vec<SceneNodeId>,
    /// The recorder every node of the pass records into, in turn: its
    /// buffer holds the fragments of the nodes on the path being painted,
    /// each after its parent's, and its canvas keeps the last shader-free
    /// paints it interned, from node to node and pass to pass. A node whose
    /// paint panics leaves it a fresh canvas.
    pub(super) recorder: PassRecorder,
    /// The most operations the recorder's buffer held at once this pass,
    /// which the pass's end hands to `fragment_room`.
    pub(super) fragment_peak: usize,
    /// The room the recorder's buffer keeps between passes.
    fragment_room: FragmentRoom,
    /// The layers of the node being painted for its own effects, collected
    /// inside its paint's containment and pushed before its children paint:
    /// empty between nodes.
    pub(super) own_effects: Vec<Effect>,
    /// The words of [`PaintedSlots`] of the nodes on the path being painted
    /// that needed them, each after its ancestors': empty between passes.
    pub(super) painted_slots: Vec<u64>,
    /// The previous chunks of the open contents that were looked up out of
    /// order.
    pub(super) chunk_index: ChunkIndex,
}

/// The recorder a pass reuses from node to node, in place: moving it per node
/// would copy its canvas each time.
#[derive(Debug)]
pub(super) struct PassRecorder(pub(super) FragmentRecorder);

impl Default for PassRecorder {
    fn default() -> Self {
        Self(FragmentRecorder::new(
            Offset::ZERO,
            flui_foundation::geometry::DevicePixelRatio::ONE,
        ))
    }
}

impl ComposerScratch {
    /// Empties every collection; each was emptied when the last pass that
    /// used it ended, so this costs nothing unless that pass failed.
    pub(super) fn clear(&mut self) {
        self.builds.clear();
        self.publishes.clear();
        self.places.clear();
        self.served.clear();
        self.slotted.clear();
        self.child_paint_orders.clear();
        self.consumed_updates.clear();
        self.visited.clear();
        self.ordinals.clear();
        self.effect_owner.clear();
        self.layer_patches.clear();
        self.republished.clear();
        self.retired.clear();
        self.retired_set.clear();
        self.unslotted.clear();
        self.records.clear();
        self.placed.clear();
        self.stale.clear();
        self.recorder.0.reset();
        self.own_effects.clear();
        self.painted_slots.clear();
        self.chunk_index.recycle(KEPT_CAPACITY);
    }
}

#[cfg(test)]
mod tests {
    use crate::context::FragmentOp;

    use super::{FRAGMENT_PASSES, FragmentRoom, KEPT_CAPACITY, KEPT_FRAGMENT_OPS, keep_room};

    fn room_after(peaks: &[usize]) -> usize {
        let mut room = FragmentRoom::default();
        peaks
            .iter()
            .map(|&peak| room.after_pass(peak))
            .last()
            .unwrap_or(0)
    }

    /// Passes over one fragment buffer, each pushing as many operations as
    /// its peak one at a time, as a paint pass records them: the times each
    /// pass grew the buffer, and the room each left behind.
    fn passes(peaks: &[usize]) -> Vec<(usize, usize)> {
        let mut room = FragmentRoom::default();
        let mut ops: Vec<FragmentOp> = Vec::new();
        peaks
            .iter()
            .map(|&peak| {
                let mut grown = 0;
                for _ in 0..peak {
                    let before = ops.capacity();
                    ops.push(FragmentOp::Pop);
                    grown += usize::from(ops.capacity() != before);
                }
                ops.clear();
                keep_room(&mut ops, room.after_pass(peak));
                (grown, ops.capacity())
            })
            .collect()
    }

    #[test]
    fn fragment_room_keeps_a_size_that_recurs() {
        // A list scrolled frame after frame.
        assert_eq!(room_after(&[300, 300]), 300);
        // A small repaint between its frames keeps the scroll's room.
        assert_eq!(room_after(&[300, 40, 300, 40]), 300);
        // Two sizes that alternate keep the larger.
        assert_eq!(room_after(&[1000, 400, 1000, 400]), 1000);
    }

    #[test]
    fn fragment_room_leaves_nothing_for_a_size_needed_once() {
        // A mount, then small passes.
        assert_eq!(room_after(&[5000, 10]), 10);
        assert_eq!(room_after(&[5000, 10, 10]), 10);
        // A size forgotten after the window passes it by.
        let mut peaks = vec![300, 300];
        peaks.extend(std::iter::repeat_n(10, FRAGMENT_PASSES - 1));
        assert_eq!(room_after(&peaks), 10);
    }

    #[test]
    fn fragment_room_never_passes_the_cap() {
        assert_eq!(room_after(&[1500, 1500]), KEPT_CAPACITY);
    }

    #[test]
    fn a_repeating_pass_grows_its_buffer_only_while_it_warms() {
        // Each row's peaks, and the pass from which none grows the buffer.
        for (peaks, warm) in [
            (vec![300; 6], 3),
            ([300, 40].repeat(4), 3),
            // A pass a little larger than the one it repeats finds room in
            // the doubled buffer.
            (vec![300, 300, 320, 300, 300, 320, 300], 3),
            (vec![1500; 6], 3),
            ([2000, 1000].repeat(4), 3),
        ] {
            let grown: Vec<usize> = passes(&peaks).iter().map(|&(grown, _)| grown).collect();
            assert!(
                grown[warm..].iter().all(|&grown| grown == 0),
                "passes {peaks:?} keep growing their buffer: {grown:?}"
            );
        }
    }

    #[test]
    fn a_pass_needed_once_leaves_no_room_and_none_past_twice_the_cap() {
        let left = passes(&[5000, 10]);
        assert!(
            left[1].1 <= KEPT_FRAGMENT_OPS,
            "a mount's room stays: {left:?}"
        );
        // Grown by doubling past twice the cap, the buffer keeps no more,
        // whether it grew from nothing or from a room a trim left between
        // powers of two (700, regrown to 2800).
        for peaks in [
            vec![2 * KEPT_CAPACITY + 1; 2],
            vec![700, 2500, 2 * KEPT_CAPACITY + 52],
        ] {
            let left = passes(&peaks);
            assert!(
                left.iter().all(|&(_, room)| room <= 2 * KEPT_CAPACITY),
                "room past twice the cap: {left:?}"
            );
        }
    }
}
