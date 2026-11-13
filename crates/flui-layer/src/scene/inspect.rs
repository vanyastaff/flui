//! The retained scene as an [`InspectTree`] (ADR-0242), read in place.
//!
//! A node of the tree is a boundary's [`SceneNode`](super::SceneNode), or an effect or a chunk of
//! its content; a slot entry is not a node of its own, the child boundary it names stands there.
//! Each is named by the boundary's slot, generation and the scene's lineage, and an entry by its
//! position in the boundary's content as well (`l4.2:3@7`, see [`SceneLayer`]). A boundary keeps
//! its node across frames, so an id read before a repaint elsewhere still names it after; one
//! kept from a retired node, from another lineage, or from an entry its content no longer holds
//! names nothing.
//!
//! A boundary reports its placement as an `Offset`, an effect its layer, and a chunk as a
//! `Picture`: its command count and fingerprint, as a picture layer reports them, linked to the
//! render object that painted it.

use std::num::NonZeroU64;

use flui_foundation::geometry::Offset;
use flui_foundation::inspect::{
    Fields, Inspect, InspectId, InspectTree, Level, Link, Links, NodeView, SceneLayer, Unit,
};

use super::content::{Bounds, Chunk, Entry, EntryIndex};
use super::id::NodeKey;
use super::version::{NodeRecord, SceneVersion};
use crate::inspect::fingerprint;

impl Inspect for NodeRecord {
    fn kind(&self) -> &'static str {
        "Offset"
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        let offset: Offset<f64> = self.placement().offset();
        out.field("offset", &offset).unit(Unit::LogicalPx);
    }
}

impl Inspect for Chunk {
    fn kind(&self) -> &'static str {
        "Picture"
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        out.field("commands", &self.commands().len());
        out.field("content", &fingerprint(self.commands()))
            .level(Level::Detail);
    }
}

/// What a [`SceneLayer`] names in one version.
#[derive(Clone, Copy)]
struct Named<'v> {
    record: &'v NodeRecord,
    entry: Option<usize>,
}

impl SceneVersion {
    /// The scene's lineage, as a layer link carries it.
    fn scene(&self) -> NonZeroU64 {
        NonZeroU64::new(self.id().lineage().get()).expect("BUG: a lineage is never zero")
    }

    /// `entry` of the node `key` names, or the node itself.
    fn link_of(&self, key: NodeKey, entry: Option<usize>) -> InspectId {
        Link::Layer(SceneLayer {
            scene: self.scene(),
            slot: key.slot(),
            generation: key.generation(),
            entry: entry.map(|entry| {
                u32::try_from(entry).expect("BUG: a boundary's entries are indexed by u32")
            }),
        })
        .into()
    }

    /// What `id` names here: a live node of this lineage, or an effect or chunk of its content.
    fn named(&self, id: InspectId) -> Option<Named<'_>> {
        let Link::Layer(layer) = id.link() else {
            return None;
        };
        if layer.scene != self.scene() {
            return None;
        }
        let record = self
            .inner()
            .record(NodeKey::new(layer.slot, layer.generation))?;
        let entry = match layer.entry {
            None => None,
            Some(entry) => {
                let entry = usize::try_from(entry).ok()?;
                match record.content().entries().get(entry)? {
                    // The child boundary stands where its slot is.
                    Entry::Slot { .. } => return None,
                    Entry::Effect { .. } | Entry::Chunk { .. } => Some(entry),
                }
            }
        };
        Some(Named { record, entry })
    }

    /// The tree node standing at `position` of `record`'s content: the entry, or the child
    /// boundary a slot names.
    fn standing(&self, record: &NodeRecord, position: usize, entry: &Entry) -> Option<InspectId> {
        match entry {
            Entry::Slot { node, .. } => self
                .node(*node)
                .map(|child| self.link_of(child.id().key(), None)),
            Entry::Effect { .. } | Entry::Chunk { .. } => {
                Some(self.link_of(record.key(), Some(position)))
            }
        }
    }
}

/// A content bound as an inspect rect: `None` when it is unbounded.
fn rect_of(bounds: Bounds) -> Option<flui_foundation::geometry::Rect<f64>> {
    match bounds {
        Bounds::Finite(rect) => Some(rect),
        Bounds::Unbounded => None,
    }
}

impl InspectTree for SceneVersion {
    type Node = dyn Inspect;

    fn root(&self) -> Option<InspectId> {
        let root = self.node(self.root())?;
        Some(self.link_of(root.id().key(), None))
    }

    fn node(&self, id: InspectId) -> Option<NodeView<'_, dyn Inspect>> {
        let Named { record, entry } = self.named(id)?;
        let layer = match id.link() {
            Link::Layer(layer) => Some(layer),
            _ => None,
        };
        let Some(position) = entry else {
            return Some(NodeView {
                object: record,
                rect: None,
                links: Links {
                    layer,
                    ..Links::default()
                },
            });
        };
        let content = record.content();
        Some(match &content.entries()[position] {
            Entry::Effect { owner, .. } => {
                let layer_of_effect = content
                    .effect_layer(EntryIndex::at(position))
                    .expect("BUG: each effect entry has a layer");
                NodeView {
                    object: layer_of_effect,
                    rect: layer_of_effect.bounds(),
                    links: Links {
                        layer,
                        render: Some(*owner),
                        ..Links::default()
                    },
                }
            }
            Entry::Chunk { chunk, .. } => NodeView {
                object: &**chunk,
                rect: rect_of(chunk.bounds()),
                links: Links {
                    layer,
                    render: Some(chunk.owner()),
                    ..Links::default()
                },
            },
            Entry::Slot { .. } => unreachable!("BUG: `named` never resolves a slot entry"),
        })
    }

    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        let named = self.named(id);
        let parent = named.and_then(|named| named.entry);
        named.into_iter().flat_map(move |Named { record, entry }| {
            // A chunk holds nothing; a node holds its top-level entries, an effect the
            // entries under it.
            let holds = entry.is_none_or(|entry| {
                matches!(record.content().entries()[entry], Entry::Effect { .. })
            });
            record
                .content()
                .entries()
                .iter()
                .enumerate()
                .filter(move |(_, candidate)| {
                    holds && candidate.parent().map(EntryIndex::get) == parent
                })
                .filter_map(move |(position, candidate)| self.standing(record, position, candidate))
        })
    }
}
