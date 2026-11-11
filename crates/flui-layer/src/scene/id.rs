//! The identities a retained scene is built from (ADR-0304 §1, §2, §7).

use flui_foundation::{AtomicIdMint, IdExhausted};
use std::num::NonZeroU64;

use flui_foundation::generation::Generation;
use flui_foundation::id::{GenId, Marker};

/// Which scene owner a version, a content version or a node id comes from.
///
/// Every [`SceneOwner`](super::SceneOwner), and every
/// [`renew`](super::SceneOwner::renew) of one, takes a lineage no other has,
/// so a sequence number that restarts never equals one from before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Lineage(NonZeroU64);

/// Lineages, minted from every thread that builds a scene owner.
static NEXT_LINEAGE: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();

impl Lineage {
    /// A lineage no other call has returned.
    ///
    /// # Errors
    ///
    /// [`IdExhausted`] once the process has minted 2^64 - 1 lineages; the
    /// mint never wraps.
    pub(crate) fn fresh() -> Result<Self, IdExhausted> {
        NEXT_LINEAGE.next().map(Self)
    }

    /// The raw value, for diagnostics.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// Which applied version of a scene this is.
///
/// Drawn from its owner's sequence, so within a lineage a later version has
/// a larger `seq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneVersionId {
    lineage: Lineage,
    seq: NonZeroU64,
}

impl SceneVersionId {
    pub(crate) fn new(lineage: Lineage, seq: NonZeroU64) -> Self {
        Self { lineage, seq }
    }

    /// The owner incarnation the version belongs to.
    #[must_use]
    pub fn lineage(self) -> Lineage {
        self.lineage
    }

    /// The version's place in its lineage's sequence.
    #[must_use]
    pub fn seq(self) -> NonZeroU64 {
        self.seq
    }
}

/// Which recording of a chunk this is (ADR-0304 §2).
///
/// A chunk keeps its content version while its render object owes no paint
/// work and its recording inputs are unchanged, and takes the next one
/// otherwise; a content version from another owner, or from before a
/// [`renew`](super::SceneOwner::renew), carries another lineage and never
/// compares equal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentVersion {
    lineage: Lineage,
    seq: NonZeroU64,
}

impl ContentVersion {
    pub(crate) fn new(lineage: Lineage, seq: NonZeroU64) -> Self {
        Self { lineage, seq }
    }

    /// The owner incarnation that minted it.
    #[must_use]
    pub fn lineage(self) -> Lineage {
        self.lineage
    }

    /// Its place in the owner's sequence.
    #[must_use]
    pub fn seq(self) -> NonZeroU64 {
        self.seq
    }
}

/// The slot domain of scene nodes.
#[derive(Debug)]
pub enum SceneNodeMarker {}

impl Marker for SceneNodeMarker {}

/// A node's slot and generation, without its lineage: how a version refers
/// to its own nodes, since every node of one version shares its lineage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeKey(GenId<SceneNodeMarker>);

impl NodeKey {
    pub(crate) fn new(slot: u32, generation: Generation) -> Self {
        Self(GenId::new_gen(slot, generation))
    }

    /// The slot the node's table entry sits at.
    pub(crate) fn slot(self) -> u32 {
        self.0.index()
    }

    pub(crate) fn generation(self) -> Generation {
        self.0.generation()
    }

    /// The node's table key.
    pub(crate) fn table_key(self) -> u64 {
        u64::from(self.slot())
    }
}

/// One boundary's node in a retained scene, kept across frames.
///
/// The owner's lineage and a generational slot key, compared by both: an id
/// from another owner, or from before a [`renew`](super::SceneOwner::renew),
/// never equals one of this owner's, so a cache keyed by it serves one scene.
/// [`SceneWriter::mint_node`](super::SceneWriter::mint_node) advances its
/// slot's generation when it issues one, so an id from a patch that was never
/// applied, or of a retired node, names nothing ever again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneNodeId {
    lineage: Lineage,
    key: NodeKey,
}

impl SceneNodeId {
    pub(crate) fn new(lineage: Lineage, key: NodeKey) -> Self {
        Self { lineage, key }
    }

    /// The owner incarnation that minted it.
    #[must_use]
    pub fn lineage(self) -> Lineage {
        self.lineage
    }

    pub(crate) fn key(self) -> NodeKey {
        self.key
    }
}
