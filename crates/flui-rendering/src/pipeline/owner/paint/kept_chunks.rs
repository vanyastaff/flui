//! Which chunks of a repainted content keep their recording (ADR-0304 §2).
//!
//! A repaint reruns every render object's paint in the content, but a chunk
//! keeps its [`ContentVersion`](flui_layer::ContentVersion) when its paint
//! records what the content's previous paint recorded there, so the engine's
//! recording cache, keyed by that version, replays it. The paint walk decides
//! that from flags, and nothing is stored for it beside the published
//! content.
//!
//! A chunk holds the runs of one paint of its owner: the commands it records
//! between the entries of its children and its effects. Those depend on the
//! owner's state, its layout, its origin in the boundary and the paint anchor
//! it reads. A change of state marks the owner, and a layout sets its
//! `LAID_OUT_SINCE_PAINT`. Its origin is the sum of the offsets its
//! ancestors in the boundary placed it at, each set by its parent's layout,
//! or by its parent's paint from that parent's state or anchor, so it is the
//! previous one while no ancestor laid out, was marked itself
//! (`PAINT_CHANGED`) since, or read its anchor: the walk passes that down as
//! `placed_as_before`. A paint that reads its anchor is not kept. Where the
//! owner's runs are cut into chunks depends on which children add entries
//! between them, and a child that starts or stops adding one is marked: its
//! mark walks up through the owner to the boundary, unless it meets a node
//! marked already, whose own mark walked the same way. Effects are entries
//! of their own, so a composited-layer update changes no chunk.
//!
//! A chunk is matched to the previous one by its owner and ordinal. Those
//! name the same run of the same paint because a run of another paint seals
//! the open chunk, so no chunk holds two paints, and a render object painted
//! a second time in a content is not placed as before: only its first paint
//! keeps chunks, whose ordinals count from zero. The device pixel ratio is
//! the last input, and a change of it drops every record before the pass, so
//! no previous content is there to keep from.

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_layer::{BoundaryContent, Chunk, Entry, Lineage};
use rustc_hash::FxHashMap;

/// The content a boundary published before the pass that repaints it, for
/// that pass to keep the unchanged chunks of.
#[derive(Debug)]
pub(super) struct PreviousChunks {
    content: Arc<BoundaryContent>,
    /// The serial of the content being built over it, which keys its
    /// entries in [`ChunkIndex`].
    serial: u64,
    /// Where the next lookup starts: chunks come back in the order they were
    /// recorded unless a parent reordered its children.
    cursor: usize,
    /// Whether [`ChunkIndex`] holds this content's chunks, which the first
    /// lookup that misses the forward scan files, so a reordered content
    /// costs one pass over it.
    indexed: bool,
}

impl PreviousChunks {
    /// The content `serial` builds over `content`.
    pub(super) fn new(serial: u64, content: &Arc<BoundaryContent>) -> Self {
        Self {
            content: Arc::clone(content),
            serial,
            cursor: 0,
            indexed: false,
        }
    }
}

/// The chunks of the previous contents a pass builds over that were looked
/// up out of order, by content serial, owner and ordinal. A pass's working
/// collection, kept between passes so it allocates only when outgrown.
#[derive(Debug, Default)]
pub(super) struct ChunkIndex(FxHashMap<(u64, RenderId, u32), usize>);

impl ChunkIndex {
    /// The chunk `owner` recorded at `ordinal` in `previous`, if it is one
    /// of `lineage`.
    pub(super) fn kept(
        &mut self,
        previous: &mut PreviousChunks,
        owner: RenderId,
        ordinal: u32,
        lineage: Lineage,
    ) -> Option<Arc<Chunk>> {
        let at = self.position(previous, owner, ordinal)?;
        match previous.content.entries().get(at) {
            Some(Entry::Chunk { chunk, .. }) if chunk.version().lineage() == lineage => {
                Some(Arc::clone(chunk))
            }
            _ => None,
        }
    }

    /// Empties the index, keeping room for `kept_capacity` entries at most:
    /// a pass after a large one finds room without allocating.
    pub(super) fn recycle(&mut self, kept_capacity: usize) {
        self.0.clear();
        self.0.shrink_to(kept_capacity);
    }

    /// Where `owner`'s chunk of `ordinal` sits in `previous`'s entries:
    /// found by a forward scan from the cursor until one misses, then by the
    /// index that miss builds.
    fn position(
        &mut self,
        previous: &mut PreviousChunks,
        owner: RenderId,
        ordinal: u32,
    ) -> Option<usize> {
        fn chunk_of(entry: &Entry) -> Option<(RenderId, u32)> {
            match entry {
                Entry::Chunk { chunk, .. } => Some((chunk.owner(), chunk.ordinal())),
                _ => None,
            }
        }
        let matches = |entry: &Entry| chunk_of(entry) == Some((owner, ordinal));
        let entries = previous.content.entries();
        let cursor = previous.cursor;
        let found = if previous.indexed {
            if entries.get(cursor).is_some_and(matches) {
                Some(cursor)
            } else {
                self.0.get(&(previous.serial, owner, ordinal)).copied()
            }
        } else if let Some(offset) = entries
            .get(cursor..)
            .and_then(|rest| rest.iter().position(matches))
        {
            Some(cursor + offset)
        } else {
            previous.indexed = true;
            let serial = previous.serial;
            self.0
                .extend(entries.iter().enumerate().filter_map(|(at, entry)| {
                    chunk_of(entry).map(|(owner, ordinal)| ((serial, owner, ordinal), at))
                }));
            self.0.get(&(serial, owner, ordinal)).copied()
        };
        let at = found?;
        previous.cursor = at + 1;
        Some(at)
    }
}

/// Which of its child slots a node's paint has painted, to tell a child it
/// paints a second time, which is not placed as before.
///
/// A parent painting its slots in rising or falling order holds only the
/// range it painted, since a slot outside it is new. The first slot inside
/// the range files every slot painted so far as one bit of the node's words
/// on the pass's stack, and the bits answer from then on, so any order costs
/// one step per child.
#[derive(Debug, Default)]
pub(super) struct PaintedSlots {
    /// The lowest and highest slot painted.
    range: Option<(usize, usize)>,
    /// Where the node's words start on the stack, once it has them.
    words_from: Option<usize>,
}

impl PaintedSlots {
    /// Records that `slot` of `slots` is painted after `painted`, the slots
    /// painted before it, and answers whether it was painted already.
    pub(super) fn paint(
        &mut self,
        stack: &mut Vec<u64>,
        slots: usize,
        painted: &[super::PaintedChild],
        slot: usize,
    ) -> bool {
        let inside = self
            .range
            .is_some_and(|(lowest, highest)| (lowest..=highest).contains(&slot));
        self.range = Some(self.range.map_or((slot, slot), |(lowest, highest)| {
            (lowest.min(slot), highest.max(slot))
        }));
        if self.words_from.is_none() {
            if !inside {
                return false;
            }
            let from = stack.len();
            stack.resize(from + slots.div_ceil(64), 0);
            for child in painted {
                set_bit(stack, from, child.slot);
            }
            self.words_from = Some(from);
        }
        self.words_from
            .is_some_and(|from| set_bit(stack, from, slot))
    }

    /// Gives the node's words back to the stack, after its children gave
    /// back theirs.
    pub(super) fn release(self, stack: &mut Vec<u64>) {
        if let Some(from) = self.words_from {
            stack.truncate(from);
        }
    }
}

/// Sets `slot`'s bit in the words from `from` and answers whether it was set;
/// a slot past them has none.
fn set_bit(stack: &mut [u64], from: usize, slot: usize) -> bool {
    let Some(word) = stack.get_mut(from + slot / 64) else {
        return false;
    };
    let bit = 1_u64 << (slot % 64);
    let was = *word & bit != 0;
    *word |= bit;
    was
}

#[cfg(test)]
mod tests {
    use flui_layer::SceneOwner;
    use flui_painting::DisplayList;

    use super::*;

    /// A content of an effect and then one chunk for each of four render
    /// objects, in order, as a repaint builds over it.
    fn previous(owner: &mut SceneOwner) -> (PreviousChunks, Vec<Arc<Chunk>>, Lineage) {
        let mut writer = owner.writer();
        let lineage = writer.current().id().lineage();
        let mut builder = BoundaryContent::builder();
        builder.push_effect(flui_layer::OffsetLayer::zero(), RenderId::new(9), 0);
        builder.pop_effect();
        let mut chunks = Vec::new();
        for slot in 0..4 {
            let version = writer.next_content_version().expect("a version");
            let chunk = Arc::new(Chunk::new(
                RenderId::new(slot + 1),
                0,
                version,
                DisplayList::new(),
            ));
            builder.chunk(Arc::clone(&chunk));
            chunks.push(chunk);
        }
        let content = Arc::new(builder.finish());
        (PreviousChunks::new(1, &content), chunks, lineage)
    }

    /// Chunks looked up in another order than they were recorded in are
    /// each found once: the forward scan misses, and the index serves the
    /// rest.
    #[test]
    fn a_reordered_content_finds_every_chunk() {
        let mut owner = SceneOwner::new();
        let mut index = ChunkIndex::default();
        let (mut previous, chunks, lineage) = previous(&mut owner);
        for at in [3, 0, 2, 1] {
            let found = index
                .kept(&mut previous, chunks[at].owner(), 0, lineage)
                .expect("the chunk is found");
            assert!(Arc::ptr_eq(&found, &chunks[at]), "chunk {at}");
        }
    }

    /// A chunk of an ordinal or an owner the content does not hold is not
    /// found, and neither is an effect's entry of the same owner and ordinal.
    #[test]
    fn a_chunk_the_content_does_not_hold_is_not_kept() {
        let mut owner = SceneOwner::new();
        let mut index = ChunkIndex::default();
        let (mut previous, chunks, lineage) = previous(&mut owner);
        let mut found = |owner: RenderId, ordinal: u32| {
            index.kept(&mut previous, owner, ordinal, lineage).is_some()
        };
        assert!(!found(chunks[2].owner(), 1));
        assert!(!found(RenderId::new(9), 0), "an effect is not a chunk");
        assert!(found(chunks[3].owner(), 0));
    }

    /// A slot painted again is told apart from a first paint whatever order
    /// the slots come in, and the stack holds words only while an order
    /// needs them.
    #[test]
    fn painted_slots_tell_a_slot_painted_again_in_any_order() {
        let mut stack = vec![7];
        for (order, again) in [
            (&[0, 1, 2, 3][..], &[false, false, false, false][..]),
            (&[3, 2, 1, 0], &[false, false, false, false]),
            (
                &[2, 0, 3, 1, 0, 70],
                &[false, false, false, false, true, false],
            ),
            (&[0, 0], &[false, true]),
            (&[5, 1, 70, 70, 3], &[false, false, false, true, false]),
        ] {
            let mut slots = PaintedSlots::default();
            let mut painted = super::super::ChildPaintOrder::new();
            for (&slot, &again) in order.iter().zip(again) {
                assert_eq!(
                    slots.paint(&mut stack, 71, &painted, slot),
                    again,
                    "slot {slot} of {order:?}"
                );
                painted.push(super::super::PaintedChild { slot, opacity: 1.0 });
            }
            slots.release(&mut stack);
            assert_eq!(stack, [7], "{order:?} gave its words back");
        }
    }
}
