//! Which chunks of a repainted content keep their recording (ADR-0304 §2).
//!
//! A repaint reruns every render object's paint in the content, but a chunk
//! keeps its [`ContentVersion`](flui_layer::ContentVersion) while its render
//! object owes no paint work and its recording inputs are unchanged, so the
//! engine's recording cache, keyed by that version, replays it. The rule is
//! the one a clean repaint boundary already lives by: every change to what a
//! render object paints marks it, and the mark walks up through each
//! ancestor to its boundary, so a node found unmarked has an unmarked
//! subtree and paints, for the same inputs, what it painted before.
//!
//! The inputs are the node's origin in the boundary and the paint anchor it
//! reads, compared bit for bit; the device pixel ratio is the third, and a
//! change of it drops every record before the pass, so no previous chunk is
//! there to keep. A chunk is matched to the previous one by its owner and
//! ordinal, which name the same run only when both chunks are runs of one
//! paint of their owner, its first that sealed a chunk in the content: a
//! chunk holding runs of two paints, or of a paint after the owner's first,
//! is recorded as one that cannot be kept.
//!
//! The lists cost no allocation once a content has been painted twice: a
//! pass seals the new list into its staged patch while it reads the previous
//! one through a shared handle it lets go of when the content ends, and
//! applying the patch writes the new list into the room of the list it
//! replaces ([`RecordedChunks::commit`]), or of one a record that left kept
//! ([`SpareChunkLists`]).

use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_layer::{BoundaryContent, Chunk, Entry, EntryIndex, Lineage};
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

use crate::context::PaintAnchor;

/// What a node's paint recorded from beyond its own state.
#[derive(Clone, Copy, Debug)]
pub(super) struct RecordingInputs {
    /// The node's origin in the boundary's space.
    pub(super) origin: Offset,
    /// What the node read as its paint anchor.
    pub(super) anchor: Option<PaintAnchor>,
}

impl RecordingInputs {
    /// Whether `self` and `other` hold the same values bit for bit: `0.0`
    /// and `-0.0` differ, and a NaN equals nothing, so a doubtful input
    /// records again.
    fn same(&self, other: &Self) -> bool {
        fn bits(offset: Offset) -> [u64; 2] {
            [offset.dx.to_bits(), offset.dy.to_bits()]
        }
        let finite = |inputs: &Self| {
            inputs.origin.is_finite()
                && inputs.anchor.is_none_or(|anchor| {
                    anchor.offset.is_finite()
                        && anchor.size.width.is_finite()
                        && anchor.size.height.is_finite()
                })
        };
        let anchors = match (self.anchor, other.anchor) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                bits(a.offset) == bits(b.offset)
                    && a.size.width.to_bits() == b.size.width.to_bits()
                    && a.size.height.to_bits() == b.size.height.to_bits()
            }
            _ => false,
        };
        finite(self) && finite(other) && anchors && bits(self.origin) == bits(other.origin)
    }
}

/// One chunk of a content a pass recorded, as the next pass that repaints
/// the content looks it up.
#[derive(Clone, Copy, Debug)]
pub(super) struct ChunkSource {
    /// The chunk's entry in the content.
    pub(super) entry: EntryIndex,
    pub(super) owner: RenderId,
    pub(super) ordinal: u32,
    /// The inputs of the one paint whose runs the chunk holds; `None` when it
    /// cannot be kept (see the module docs).
    pub(super) inputs: Option<RecordingInputs>,
}

/// The chunks of a content a record holds, in paint order.
#[derive(Clone, Debug)]
pub(super) enum RecordedChunks {
    /// Sealed by a pass whose patch is not applied yet: the range of the
    /// staged patch's list they sit at.
    Staged(Range<usize>),
    /// None or one, the chunks of most boundaries, held in the record.
    Inline(Option<ChunkSource>),
    /// Two or more, in the record's own list, shared with the pass that
    /// repaints the content while it builds over them.
    Shared(Rc<Vec<ChunkSource>>),
}

impl RecordedChunks {
    /// The chunks once the patch that staged them is applied; none before.
    fn committed(&self) -> &[ChunkSource] {
        match self {
            Self::Inline(chunk) => chunk.as_slice(),
            Self::Shared(list) => list,
            Self::Staged(_) => {
                debug_assert!(false, "BUG: a committed record holds its chunks");
                &[]
            }
        }
    }

    /// Makes a staged list the record's own, from the patch's `staged`
    /// list: in the record when it holds one chunk at most, else in the
    /// room of the list of the record it replaces or of a spare one. The
    /// pass that built over the replaced list let go of it when its content
    /// ended, so the list is the record's alone.
    pub(super) fn commit(
        &mut self,
        staged: &[ChunkSource],
        replaced: Option<Self>,
        spares: &mut SpareChunkLists,
        kept_capacity: usize,
    ) {
        let Self::Staged(range) = self else {
            return;
        };
        let staged = staged.get(range.clone()).unwrap_or_default();
        if let [] | [_] = staged {
            *self = Self::Inline(staged.first().copied());
            if let Some(replaced) = replaced {
                spares.keep(replaced, kept_capacity);
            }
            return;
        }
        let mut list = match replaced {
            Some(Self::Shared(list)) => list,
            _ => spares.take(staged.len()),
        };
        if let Some(room) = Rc::get_mut(&mut list) {
            room.clear();
            room.extend_from_slice(staged);
        } else {
            list = Rc::new(staged.to_vec());
        }
        *self = Self::Shared(list);
    }
}

/// How many lists [`SpareChunkLists`] keeps, inline.
const SPARE_LISTS: usize = 64;

/// The lists of records that left, for records that arrive to be written
/// into: a list scrolled out of view serves one scrolled in. Bounded by
/// count and by each list's room.
#[derive(Debug, Default)]
pub(super) struct SpareChunkLists(SmallVec<[Rc<Vec<ChunkSource>>; SPARE_LISTS]>);

impl SpareChunkLists {
    /// Keeps the list of a record that left, if it is the record's alone
    /// and there is room for it.
    pub(super) fn keep(&mut self, chunks: RecordedChunks, kept_capacity: usize) {
        let RecordedChunks::Shared(mut list) = chunks else {
            return;
        };
        let Some(room) = Rc::get_mut(&mut list) else {
            return;
        };
        if room.capacity() == 0 || room.capacity() > kept_capacity || self.0.len() >= SPARE_LISTS {
            return;
        }
        room.clear();
        self.0.push(list);
    }

    /// A spare list with room for `len` sources, or a new one.
    fn take(&mut self, len: usize) -> Rc<Vec<ChunkSource>> {
        self.0
            .iter()
            .position(|list| list.capacity() >= len)
            .map_or_else(Rc::default, |at| self.0.swap_remove(at))
    }
}

/// The content a boundary published before the pass that repaints it, and
/// its chunks, for that pass to keep the unchanged ones of.
#[derive(Debug)]
pub(super) struct PreviousChunks {
    content: Arc<BoundaryContent>,
    sources: RecordedChunks,
    /// The serial of the content being built over it, which keys its
    /// entries in [`ChunkIndex`].
    serial: u64,
    /// Where the next lookup starts: chunks come back in the order they were
    /// recorded unless a parent reordered its children.
    cursor: usize,
    /// Whether [`ChunkIndex`] holds this list, which the first lookup that
    /// misses the forward scan builds, so a reordered content costs one
    /// pass over it.
    indexed: bool,
}

impl PreviousChunks {
    /// The content `serial` builds over `content`, published before with
    /// `chunks`; `None` when it has no chunk to keep.
    pub(super) fn new(
        serial: u64,
        content: &Arc<BoundaryContent>,
        chunks: &RecordedChunks,
    ) -> Option<Self> {
        if chunks.committed().is_empty() {
            return None;
        }
        Some(Self {
            content: Arc::clone(content),
            sources: chunks.clone(),
            serial,
            cursor: 0,
            indexed: false,
        })
    }
}

/// The chunks of the contents a pass builds that were looked up out of
/// order, by content serial, owner and ordinal. A pass's working
/// collection, kept between passes so it allocates only when outgrown.
#[derive(Debug, Default)]
pub(super) struct ChunkIndex(FxHashMap<(u64, RenderId, u32), usize>);

impl ChunkIndex {
    /// The chunk of `previous` that `owner` recorded at `ordinal` from
    /// `inputs`, if there is one of `lineage` that can be kept.
    pub(super) fn kept(
        &mut self,
        previous: &mut PreviousChunks,
        owner: RenderId,
        ordinal: u32,
        inputs: &RecordingInputs,
        lineage: Lineage,
    ) -> Option<Arc<Chunk>> {
        let at = self.position(previous, owner, ordinal)?;
        let source = previous.sources.committed().get(at)?;
        if !source.inputs.is_some_and(|recorded| recorded.same(inputs)) {
            return None;
        }
        match previous.content.entries().get(source.entry.get()) {
            Some(Entry::Chunk { chunk, .. })
                if chunk.owner() == owner
                    && chunk.ordinal() == ordinal
                    && chunk.version().lineage() == lineage =>
            {
                Some(Arc::clone(chunk))
            }
            _ => {
                debug_assert!(
                    false,
                    "BUG: a chunk source names a chunk of another owner, ordinal or lineage"
                );
                None
            }
        }
    }

    /// Empties the index, keeping room for `kept_capacity` entries at most:
    /// a pass after a large one finds room without allocating.
    pub(super) fn recycle(&mut self, kept_capacity: usize) {
        self.0.clear();
        self.0.shrink_to(kept_capacity);
    }

    /// Where `owner`'s chunk of `ordinal` sits in `previous`'s list: found
    /// by a forward scan from the cursor until one misses, then by the
    /// index that miss builds.
    fn position(
        &mut self,
        previous: &mut PreviousChunks,
        owner: RenderId,
        ordinal: u32,
    ) -> Option<usize> {
        let matches = |source: &ChunkSource| source.owner == owner && source.ordinal == ordinal;
        let sources = previous.sources.committed();
        let cursor = previous.cursor;
        let found = if previous.indexed {
            if sources.get(cursor).is_some_and(matches) {
                Some(cursor)
            } else {
                self.0.get(&(previous.serial, owner, ordinal)).copied()
            }
        } else if let Some(offset) = sources
            .get(cursor..)
            .and_then(|rest| rest.iter().position(matches))
        {
            Some(cursor + offset)
        } else {
            previous.indexed = true;
            let serial = previous.serial;
            self.0.extend(
                sources
                    .iter()
                    .enumerate()
                    .map(|(at, source)| ((serial, source.owner, source.ordinal), at)),
            );
            self.0.get(&(serial, owner, ordinal)).copied()
        };
        let at = found?;
        previous.cursor = at + 1;
        Some(at)
    }
}

#[cfg(test)]
mod tests {
    use flui_layer::SceneOwner;
    use flui_painting::DisplayList;

    use super::*;

    const AT_ORIGIN: RecordingInputs = RecordingInputs {
        origin: Offset::ZERO,
        anchor: None,
    };

    /// A content of one chunk for each of four render objects, in order,
    /// as a repaint builds over it.
    fn previous(owner: &mut SceneOwner) -> (PreviousChunks, Vec<Arc<Chunk>>, Lineage) {
        let mut writer = owner.writer();
        let lineage = writer.current().id().lineage();
        let mut builder = BoundaryContent::builder();
        let mut sources = Vec::new();
        let mut chunks = Vec::new();
        for slot in 0..4 {
            let version = writer.next_content_version().expect("a version");
            let chunk = Arc::new(Chunk::new(
                RenderId::new(slot + 1),
                0,
                version,
                DisplayList::new(),
            ));
            let entry = builder.chunk(Arc::clone(&chunk));
            sources.push(ChunkSource {
                entry,
                owner: chunk.owner(),
                ordinal: 0,
                inputs: Some(AT_ORIGIN),
            });
            chunks.push(chunk);
        }
        let content = Arc::new(builder.finish());
        let recorded = RecordedChunks::Shared(Rc::new(sources));
        let previous = PreviousChunks::new(1, &content, &recorded).expect("four chunks");
        (previous, chunks, lineage)
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
                .kept(&mut previous, chunks[at].owner(), 0, &AT_ORIGIN, lineage)
                .expect("the chunk is found");
            assert!(Arc::ptr_eq(&found, &chunks[at]), "chunk {at}");
        }
    }

    /// A chunk recorded from other inputs, or of an ordinal the content does
    /// not hold, is not kept.
    #[test]
    fn a_chunk_of_other_inputs_or_ordinal_is_not_kept() {
        let mut owner = SceneOwner::new();
        let mut index = ChunkIndex::default();
        let (mut previous, chunks, lineage) = previous(&mut owner);
        let moved = RecordingInputs {
            origin: Offset::new(0.5, 0.0),
            anchor: None,
        };
        let negative_zero = RecordingInputs {
            origin: Offset::new(-0.0, 0.0),
            anchor: None,
        };
        let mut found = |at: usize, ordinal: u32, inputs: &RecordingInputs| {
            index
                .kept(&mut previous, chunks[at].owner(), ordinal, inputs, lineage)
                .is_some()
        };
        assert!(!found(0, 0, &moved));
        assert!(!found(1, 0, &negative_zero));
        assert!(!found(2, 1, &AT_ORIGIN));
        assert!(found(3, 0, &AT_ORIGIN));
    }

    /// A list committed over a replaced record's holds the staged range, in
    /// the replaced list's room while nothing else holds it, and in a list
    /// of its own when the replaced one is still shared.
    #[test]
    fn a_committed_list_takes_the_replaced_lists_room_when_it_is_alone() {
        let source = |slot: usize| ChunkSource {
            entry: EntryIndex::try_from(0usize).expect("index 0"),
            owner: RenderId::new(slot),
            ordinal: 0,
            inputs: None,
        };
        let staged = [source(1), source(2), source(3)];
        let commit = |replaced: Rc<Vec<ChunkSource>>| {
            let mut chunks = RecordedChunks::Staged(1..3);
            chunks.commit(
                &staged,
                Some(RecordedChunks::Shared(replaced)),
                &mut SpareChunkLists::default(),
                usize::MAX,
            );
            let RecordedChunks::Shared(list) = chunks else {
                panic!("two chunks are a shared list: {chunks:?}");
            };
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].owner, RenderId::new(2));
            list
        };
        let mut alone = Vec::with_capacity(8);
        alone.push(source(9));
        let alone = Rc::new(alone);
        let room = Rc::as_ptr(&alone);
        let written = commit(alone);
        assert_eq!(
            Rc::as_ptr(&written),
            room,
            "written in the replaced list's room"
        );

        let shared = Rc::new(vec![source(9)]);
        let written = commit(Rc::clone(&shared));
        assert!(
            !Rc::ptr_eq(&written, &shared),
            "a shared list is not written"
        );
        assert_eq!(shared[0].owner, RenderId::new(9), "nor changed");
    }
}
