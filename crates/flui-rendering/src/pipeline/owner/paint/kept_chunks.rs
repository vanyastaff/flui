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

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Offset;
use flui_layer::{BoundaryContent, Chunk, Entry, EntryIndex, Lineage};
use rustc_hash::FxHashMap;

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

/// The content a boundary published before the pass that repaints it, and
/// its chunks, for that pass to keep the unchanged ones of.
#[derive(Debug)]
pub(super) struct PreviousChunks {
    content: Arc<BoundaryContent>,
    sources: Arc<[ChunkSource]>,
    /// Where the next lookup starts: chunks come back in the order they were
    /// recorded unless a parent reordered its children.
    cursor: usize,
    /// Each source by its key, built the first time a lookup misses the
    /// forward scan, so a reordered content costs one pass over it.
    by_key: Option<FxHashMap<(RenderId, u32), usize>>,
}

impl PreviousChunks {
    pub(super) fn new(content: Arc<BoundaryContent>, sources: Arc<[ChunkSource]>) -> Self {
        Self {
            content,
            sources,
            cursor: 0,
            by_key: None,
        }
    }

    /// The previous chunk `owner` recorded at `ordinal` from `inputs`, if
    /// there is one of `lineage` that can be kept.
    pub(super) fn kept(
        &mut self,
        owner: RenderId,
        ordinal: u32,
        inputs: &RecordingInputs,
        lineage: Lineage,
    ) -> Option<Arc<Chunk>> {
        let at = self.position(owner, ordinal)?;
        let source = self.sources.get(at)?;
        if !source.inputs.is_some_and(|recorded| recorded.same(inputs)) {
            return None;
        }
        match self.content.entries().get(source.entry.get()) {
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

    fn position(&mut self, owner: RenderId, ordinal: u32) -> Option<usize> {
        let matches = |source: &ChunkSource| source.owner == owner && source.ordinal == ordinal;
        let at = if let Some(by_key) = &self.by_key {
            if self.sources.get(self.cursor).is_some_and(matches) {
                Some(self.cursor)
            } else {
                by_key.get(&(owner, ordinal)).copied()
            }
        } else {
            let scanned = self
                .sources
                .get(self.cursor..)
                .and_then(|rest| rest.iter().position(matches))
                .map(|offset| self.cursor + offset);
            scanned.or_else(|| {
                let by_key = self.by_key.insert(
                    self.sources
                        .iter()
                        .enumerate()
                        .map(|(at, source)| ((source.owner, source.ordinal), at))
                        .collect::<FxHashMap<(RenderId, u32), usize>>(),
                );
                by_key.get(&(owner, ordinal)).copied()
            })
        }?;
        self.cursor = at + 1;
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
    /// with its chunk sources.
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
        (
            PreviousChunks::new(content, Arc::<[ChunkSource]>::from(sources)),
            chunks,
            lineage,
        )
    }

    /// Chunks looked up in another order than they were recorded in are
    /// each found once: the forward scan misses, and the index serves the
    /// rest.
    #[test]
    fn a_reordered_content_finds_every_chunk() {
        let mut owner = SceneOwner::new();
        let (mut previous, chunks, lineage) = previous(&mut owner);
        for at in [3, 0, 2, 1] {
            let kept = previous
                .kept(chunks[at].owner(), 0, &AT_ORIGIN, lineage)
                .expect("the chunk is found");
            assert!(Arc::ptr_eq(&kept, &chunks[at]), "chunk {at}");
        }
    }

    /// A chunk recorded from other inputs, or of an ordinal the content does
    /// not hold, is not kept.
    #[test]
    fn a_chunk_of_other_inputs_or_ordinal_is_not_kept() {
        let mut owner = SceneOwner::new();
        let (mut previous, chunks, lineage) = previous(&mut owner);
        let moved = RecordingInputs {
            origin: Offset::new(0.5, 0.0),
            anchor: None,
        };
        let negative_zero = RecordingInputs {
            origin: Offset::new(-0.0, 0.0),
            anchor: None,
        };
        assert!(
            previous
                .kept(chunks[0].owner(), 0, &moved, lineage)
                .is_none()
        );
        assert!(
            previous
                .kept(chunks[1].owner(), 0, &negative_zero, lineage)
                .is_none()
        );
        assert!(
            previous
                .kept(chunks[2].owner(), 1, &AT_ORIGIN, lineage)
                .is_none()
        );
        assert!(
            previous
                .kept(chunks[3].owner(), 0, &AT_ORIGIN, lineage)
                .is_some()
        );
    }
}
