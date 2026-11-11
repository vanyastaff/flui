//! What one boundary paints (ADR-0304 §2).

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Rect;
use flui_painting::Paint;
use flui_painting::display_list::{DisplayList, DrawOp};
use flui_painting::paint::TextureId;
use smallvec::SmallVec;

use super::id::{ContentVersion, NodeKey, SceneNodeId};
use crate::layer::Layer;
use crate::link::LayerLink;

/// An entry's position in its boundary's paint-order stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryIndex(u32);

impl EntryIndex {
    pub(crate) fn at(position: usize) -> Self {
        Self(u32::try_from(position).expect(
            "BUG: a boundary's entries are indexed by u32; 2^32 entries never fit in memory",
        ))
    }

    /// The position in [`BoundaryContent::entries`].
    #[must_use]
    pub fn get(self) -> usize {
        // u32 to usize widens on every target FLUI builds for.
        self.0 as usize
    }
}

/// Where a recording or an effect reaches, in its own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bounds {
    /// Inside the rect.
    Finite(Rect<f64>),
    /// Anywhere its clip allows, or not finite.
    Unbounded,
}

/// One render object's sealed paint run (ADR-0304 §2).
///
/// A chunk has one owner, so a render object that repaints changes only its
/// own chunks, and one parent entry, so it never straddles an effect scope.
#[derive(Debug, Clone)]
pub struct Chunk {
    owner: RenderId,
    ordinal: u32,
    version: ContentVersion,
    commands: DisplayList,
    bounds: Bounds,
    textures: SmallVec<[TextureId; 1]>,
    reads_destination: bool,
}

impl Chunk {
    /// The `ordinal`th run `owner` painted in its boundary, recorded as
    /// `version`. Its bounds and the textures it draws are read from
    /// `commands`.
    #[must_use]
    pub fn new(
        owner: RenderId,
        ordinal: u32,
        version: ContentVersion,
        commands: DisplayList,
    ) -> Self {
        let bounds = match commands.damage_extent() {
            None => Bounds::Finite(Rect::ZERO),
            Some(extent) => match extent.covering_rect() {
                Some(rect) if rect.is_finite() => Bounds::Finite(rect),
                _ => Bounds::Unbounded,
            },
        };
        let mut textures = SmallVec::<[TextureId; 1]>::new();
        let mut destination = false;
        for command in &commands {
            if let DrawOp::Texture { texture_id, .. } = command.op
                && !textures.contains(&texture_id)
            {
                textures.push(texture_id);
            }
            destination |= reads_destination(&command.op);
        }
        Self {
            owner,
            ordinal,
            version,
            commands,
            bounds,
            textures,
            reads_destination: destination,
        }
    }

    /// Whether a command blends by a mode that reads the pixels under it, so
    /// the chunk reads and writes its whole bounds (ADR-0304 §6).
    #[must_use]
    pub fn reads_destination(&self) -> bool {
        self.reads_destination
    }

    /// The render object that painted it.
    #[must_use]
    pub fn owner(&self) -> RenderId {
        self.owner
    }

    /// Which of its owner's runs in the boundary it is.
    #[must_use]
    pub fn ordinal(&self) -> u32 {
        self.ordinal
    }

    /// Which recording it is.
    #[must_use]
    pub fn version(&self) -> ContentVersion {
        self.version
    }

    /// The recorded commands.
    #[must_use]
    pub fn commands(&self) -> &DisplayList {
        &self.commands
    }

    /// Every pixel replaying it can change, in the boundary's space.
    #[must_use]
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// The external textures it draws, each once.
    #[must_use]
    pub fn textures(&self) -> &[TextureId] {
        &self.textures
    }
}

/// One entry of a boundary's paint-order stream (ADR-0253 §1).
///
/// `parent` is the effect the entry sits under, always an earlier entry of
/// the same content; `None` is the boundary itself.
#[derive(Debug, Clone)]
pub enum Entry {
    /// A layer the boundary's paint pushed, which its children sit under.
    Effect {
        /// The layer.
        layer: Layer,
        /// The render object that pushed it.
        owner: RenderId,
        /// Which of its owner's effects in the boundary it is.
        ordinal: u32,
        /// The effect it sits under.
        parent: Option<EntryIndex>,
    },
    /// A sealed paint run.
    Chunk {
        /// The run.
        chunk: Arc<Chunk>,
        /// The effect it sits under.
        parent: Option<EntryIndex>,
    },
    /// A child boundary, painted where this entry stands.
    Slot {
        /// The child's node.
        node: SceneNodeId,
        /// The effect it sits under.
        parent: Option<EntryIndex>,
    },
}

impl Entry {
    /// The effect the entry sits under.
    #[must_use]
    pub fn parent(&self) -> Option<EntryIndex> {
        match self {
            Self::Effect { parent, .. }
            | Self::Chunk { parent, .. }
            | Self::Slot { parent, .. } => *parent,
        }
    }
}

/// Whether a link's user publishes the anchor or positions against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkRole {
    Leader,
    Follower,
}

/// What one boundary paints: one paint-order stream of effects, chunks and
/// child slots, built by a [`ContentBuilder`] so every entry's parent is an
/// earlier effect.
#[derive(Debug)]
pub struct BoundaryContent {
    entries: Box<[Entry]>,
    /// Each link layer the entries hold.
    links: Box<[(LayerLink, LinkRole, EntryIndex)]>,
    /// Each entry that draws an external texture, by texture.
    textures: Box<[(TextureId, EntryIndex)]>,
    /// Each slot's entry, by its node's key.
    slot_entries: Box<[(SceneNodeId, EntryIndex)]>,
    /// Whether an entry reads pixels other than those it draws: a filter,
    /// a colour filter painting transparent pixels, a chunk that reads the
    /// destination.
    filtered: bool,
}

impl BoundaryContent {
    /// Starts an empty stream.
    #[must_use]
    pub fn builder() -> ContentBuilder {
        ContentBuilder::default()
    }

    /// The stream in paint order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The child boundaries the stream names, in paint order.
    pub fn slots(&self) -> impl Iterator<Item = SceneNodeId> + '_ {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Slot { node, .. } => Some(*node),
            Entry::Effect { .. } | Entry::Chunk { .. } => None,
        })
    }

    pub(crate) fn links(&self) -> &[(LayerLink, LinkRole, EntryIndex)] {
        &self.links
    }

    pub(crate) fn textures(&self) -> &[(TextureId, EntryIndex)] {
        &self.textures
    }

    /// The slot that names `child`, if one does.
    pub(crate) fn slot_entry(&self, child: NodeKey) -> Option<EntryIndex> {
        let at = self
            .slot_entries
            .binary_search_by_key(&child.table_key(), |(node, _)| node.key().table_key())
            .ok()?;
        let (node, entry) = self.slot_entries[at];
        (node.key() == child).then_some(entry)
    }

    pub(crate) fn is_filtered(&self) -> bool {
        self.filtered
    }

    /// Whether `other` puts the same entries into the three indexes.
    pub(crate) fn same_indexes(&self, other: &Self) -> bool {
        self.links == other.links
            && self.textures == other.textures
            && self.filtered == other.filtered
    }
}

impl Default for BoundaryContent {
    fn default() -> Self {
        ContentBuilder::default().finish()
    }
}

/// Builds a [`BoundaryContent`] in paint order: an effect opens a scope the
/// entries after it sit in until it is closed.
#[derive(Debug, Default)]
pub struct ContentBuilder {
    entries: Vec<Entry>,
    open: Vec<EntryIndex>,
}

impl ContentBuilder {
    /// Pushes `layer` under the open effect and opens it.
    pub fn push_effect(&mut self, layer: Layer, owner: RenderId, ordinal: u32) -> EntryIndex {
        let index = self.push(|parent| Entry::Effect {
            layer,
            owner,
            ordinal,
            parent,
        });
        self.open.push(index);
        index
    }

    /// Closes the innermost open effect, `None` when none is open.
    pub fn pop_effect(&mut self) -> Option<EntryIndex> {
        self.open.pop()
    }

    /// Adds `chunk` under the open effect.
    pub fn chunk(&mut self, chunk: Arc<Chunk>) -> EntryIndex {
        self.push(|parent| Entry::Chunk { chunk, parent })
    }

    /// Adds a slot for the child boundary `node` under the open effect.
    pub fn slot(&mut self, node: SceneNodeId) -> EntryIndex {
        self.push(|parent| Entry::Slot { node, parent })
    }

    /// The content, with every open effect closed.
    #[must_use]
    pub fn finish(self) -> BoundaryContent {
        let mut links = Vec::new();
        let mut textures = Vec::new();
        let mut slots = Vec::new();
        let mut filtered = false;
        for (position, entry) in self.entries.iter().enumerate() {
            let index = EntryIndex::at(position);
            filtered |= entry_has_closure(entry);
            match entry {
                Entry::Effect { layer, .. } => match layer {
                    Layer::Leader(leader) => links.push((leader.link(), LinkRole::Leader, index)),
                    Layer::Follower(follower) => {
                        links.push((follower.link(), LinkRole::Follower, index));
                    }
                    Layer::Texture(texture) => textures.push((texture.texture_id(), index)),
                    _ => {}
                },
                Entry::Chunk { chunk, .. } => {
                    textures.extend(chunk.textures().iter().map(|texture| (*texture, index)));
                }
                Entry::Slot { node, .. } => slots.push((*node, index)),
            }
        }
        slots.sort_unstable_by_key(|(node, _)| node.key().table_key());
        BoundaryContent {
            entries: self.entries.into_boxed_slice(),
            links: links.into_boxed_slice(),
            textures: textures.into_boxed_slice(),
            slot_entries: slots.into_boxed_slice(),
            filtered,
        }
    }

    fn push(&mut self, entry: impl FnOnce(Option<EntryIndex>) -> Entry) -> EntryIndex {
        let index = EntryIndex::at(self.entries.len());
        self.entries.push(entry(self.open.last().copied()));
        index
    }
}

/// Whether `op` blends with what is under it by a mode that reads the
/// destination beyond the source's own coverage (ADR-0304 §6).
fn reads_destination(op: &DrawOp) -> bool {
    let paint = |paint: &Paint| paint.blend_mode.is_advanced();
    match op {
        DrawOp::Line { paint: p, .. }
        | DrawOp::Rect { paint: p, .. }
        | DrawOp::RRect { paint: p, .. }
        | DrawOp::Circle { paint: p, .. }
        | DrawOp::Oval { paint: p, .. }
        | DrawOp::Path { paint: p, .. }
        | DrawOp::Arc { paint: p, .. }
        | DrawOp::DRRect { paint: p, .. }
        | DrawOp::Points { paint: p, .. }
        | DrawOp::Vertices { paint: p, .. }
        | DrawOp::Paint { paint: p }
        | DrawOp::SaveLayer { paint: p, .. } => paint(p),
        DrawOp::Image { paint: p, .. }
        | DrawOp::ImageRegion { paint: p, .. }
        | DrawOp::ImageRepeat { paint: p, .. }
        | DrawOp::ImageNineSlice { paint: p, .. }
        | DrawOp::ImageFiltered { paint: p, .. } => p.as_deref().is_some_and(paint),
        DrawOp::Color { blend_mode, .. } => blend_mode.is_advanced(),
        DrawOp::Atlas {
            blend_mode,
            paint: p,
            ..
        } => blend_mode.is_advanced() || p.as_deref().is_some_and(paint),
        DrawOp::ClipRect { .. }
        | DrawOp::ClipRRect { .. }
        | DrawOp::ClipRSuperellipse { .. }
        | DrawOp::ClipPath { .. }
        | DrawOp::Paragraph { .. }
        | DrawOp::ParagraphShadow { .. }
        | DrawOp::Texture { .. }
        | DrawOp::Shadow { .. }
        | DrawOp::RestoreLayer
        | DrawOp::Save
        | DrawOp::Restore => false,
    }
}

/// Whether `entry` reads pixels other than those it draws, so damage that
/// meets it takes its read closure (ADR-0304 §6).
fn entry_has_closure(entry: &Entry) -> bool {
    match entry {
        Entry::Effect { layer, .. } => match layer {
            Layer::BackdropFilter(_) | Layer::ImageFilter(_) => true,
            Layer::ColorFilter(filter) => filter.color_filter().modifies_transparent_black(),
            _ => false,
        },
        Entry::Chunk { chunk, .. } => chunk.reads_destination(),
        Entry::Slot { .. } => false,
    }
}
