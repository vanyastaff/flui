//! What one boundary paints (ADR-0304 §2).

use std::sync::Arc;

use flui_foundation::RenderId;
use flui_foundation::geometry::Rect;
use flui_painting::Paint;
use flui_painting::display_list::{DisplayList, DrawOp};
use flui_painting::paint::TextureId;
use smallvec::SmallVec;

use super::geometry::Reach;
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

impl TryFrom<usize> for EntryIndex {
    type Error = std::num::TryFromIntError;

    /// The entry at `position` of [`BoundaryContent::entries`].
    fn try_from(position: usize) -> Result<Self, Self::Error> {
        u32::try_from(position).map(Self)
    }
}

/// Where a recording or an effect reaches, in its own space.
#[derive(Debug, Clone, Copy, PartialEq)]
#[expect(clippy::exhaustive_enums, reason = "a reach is finite or it is not")]
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
    commands: Arc<DisplayList>,
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
        let mut chunk = Self {
            owner,
            ordinal,
            version,
            commands: Arc::new(commands),
            bounds: Bounds::Finite(Rect::ZERO),
            textures: SmallVec::new(),
            reads_destination: false,
        };
        chunk.measure();
        chunk
    }

    /// Becomes the `ordinal`th run `owner` painted, recorded as `version`,
    /// holding `commands`, and leaves in `commands` the emptied list it held:
    /// how a chunk the owner reclaimed is written again
    /// ([`SceneWriter::chunk`](super::SceneWriter::chunk)). `None` when its
    /// commands are shared, as a reclaimed chunk's never are.
    pub(crate) fn refill(
        &mut self,
        owner: RenderId,
        ordinal: u32,
        version: ContentVersion,
        commands: &mut DisplayList,
    ) -> Option<()> {
        let held = Arc::get_mut(&mut self.commands)?;
        std::mem::swap(held, commands);
        self.owner = owner;
        self.ordinal = ordinal;
        self.version = version;
        self.measure();
        Some(())
    }

    /// Lets go of its commands, keeping their room, when nothing else holds
    /// them: what a reclaimed chunk is kept as. The commands drop here, so a
    /// caller runs this with no borrow held.
    pub(crate) fn empty(&mut self) -> bool {
        let Some(held) = Arc::get_mut(&mut self.commands) else {
            return false;
        };
        held.clear();
        self.textures.clear();
        self.bounds = Bounds::Finite(Rect::ZERO);
        self.reads_destination = false;
        true
    }

    /// The commands' room: what recording into the chunk again saves.
    pub(crate) fn room(&self) -> usize {
        self.commands.room()
    }

    /// Reads its bounds, the textures it draws and whether it reads the
    /// destination from its commands.
    fn measure(&mut self) {
        let commands = &*self.commands;
        self.bounds = match commands.damage_extent() {
            None => Bounds::Finite(Rect::ZERO),
            Some(extent) => match extent.covering_rect() {
                Some(rect) if rect.is_finite() => Bounds::Finite(rect),
                _ => Bounds::Unbounded,
            },
        };
        self.textures.clear();
        let mut destination = false;
        for command in commands {
            if let DrawOp::Texture { texture_id, .. } = command.op
                && !self.textures.contains(&texture_id)
            {
                self.textures.push(texture_id);
            }
            destination |= reads_destination(&command.op);
        }
        self.reads_destination = destination;
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

    /// The recorded commands, shared: a picture built from the chunk holds
    /// them without copying.
    #[must_use]
    pub fn shared_commands(&self) -> &Arc<DisplayList> {
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
/// the same content; `None` is the boundary itself. An effect's layer is
/// held beside the stream ([`BoundaryContent::effect_layer`]), so replacing
/// it shares the stream.
#[derive(Debug, Clone)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a walk over a boundary's stream that skipped a new kind of entry would lose its paint"
)]
pub enum Entry {
    /// A layer the boundary's paint pushed, which its children sit under.
    Effect {
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

/// What one effect's scope holds that its layers never change.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EffectScope {
    /// The position after its scope.
    pub(crate) end: usize,
    /// The union of what its direct chunk children draw, in its children's
    /// space.
    pub(crate) chunks: Reach,
}

/// Everything of a content that depends only on its entries, shared by every
/// content an effect update derives from it.
#[derive(Debug)]
struct Stream {
    entries: Vec<Entry>,
    /// The entry of each effect, ascending: the effect at `effects[i]` has
    /// the layer `layers[i]` of the content.
    effects: Vec<EntryIndex>,
    /// Each effect's scope, as `effects` orders them.
    scopes: Vec<EffectScope>,
    /// Each effect and slot entry, ascending.
    structural: Vec<EntryIndex>,
    /// The union of what the top-level chunks draw, in the boundary's space.
    top_chunks: Reach,
    /// Each slot's entry, by its node's key.
    slot_entries: Vec<(SceneNodeId, EntryIndex)>,
    /// Each chunk that draws an external texture, by texture.
    chunk_textures: Vec<(TextureId, EntryIndex)>,
    /// Each chunk that reads the destination, in paint order.
    chunk_closures: Vec<EntryIndex>,
}

impl Stream {
    fn of(entries: Vec<Entry>) -> Self {
        let mut effects = Vec::new();
        let mut scopes: Vec<EffectScope> = Vec::new();
        let mut structural = Vec::new();
        let mut top_chunks = Reach::Nothing;
        let mut slots = Vec::new();
        let mut chunk_textures = Vec::new();
        let mut chunk_closures = Vec::new();
        // The effects the current entry sits under, innermost last, as
        // positions in `effects`.
        let mut open: SmallVec<[usize; 8]> = SmallVec::new();
        for (position, entry) in entries.iter().enumerate() {
            let index = EntryIndex::at(position);
            let parent = entry.parent();
            while let Some(&top) = open.last()
                && Some(effects[top]) != parent
            {
                scopes[top].end = position;
                open.pop();
            }
            let held = open.last().copied();
            debug_assert_eq!(
                held.map(|top| effects[top]),
                parent,
                "BUG: an entry's parent is an open effect"
            );
            match entry {
                Entry::Effect { .. } => {
                    open.push(effects.len());
                    effects.push(index);
                    scopes.push(EffectScope {
                        end: entries.len(),
                        chunks: Reach::Nothing,
                    });
                    structural.push(index);
                }
                Entry::Chunk { chunk, .. } => {
                    let reach = Reach::of(chunk.bounds());
                    match held {
                        Some(top) => scopes[top].chunks = scopes[top].chunks.union(reach),
                        None => top_chunks = top_chunks.union(reach),
                    }
                    chunk_textures.extend(chunk.textures().iter().map(|texture| (*texture, index)));
                    if chunk.reads_destination() {
                        chunk_closures.push(index);
                    }
                }
                Entry::Slot { node, .. } => {
                    slots.push((*node, index));
                    structural.push(index);
                }
            }
        }
        slots.sort_unstable_by_key(|(node, _)| node.key().table_key());
        Self {
            entries,
            effects,
            scopes,
            structural,
            top_chunks,
            slot_entries: slots,
            chunk_textures,
            chunk_closures,
        }
    }
}

/// What the layers of a content put into its indexes.
#[derive(Debug, Default)]
struct LayerIndexes {
    /// Each link layer, in paint order.
    links: Vec<(LayerLink, LinkRole, EntryIndex)>,
    /// Each texture layer, by texture, in paint order.
    textures: Vec<(TextureId, EntryIndex)>,
    /// Each effect that reads pixels other than those it draws, in paint
    /// order.
    closures: Vec<EntryIndex>,
}

impl LayerIndexes {
    fn of(effects: &[EntryIndex], layers: &[Layer]) -> Self {
        let mut indexes = Self::default();
        indexes.refill(effects, layers);
        indexes
    }

    /// Rebuilds the indexes for `layers` in the vectors already held.
    fn refill(&mut self, effects: &[EntryIndex], layers: &[Layer]) {
        self.clear();
        for (&index, layer) in effects.iter().zip(layers) {
            match layer {
                Layer::Leader(leader) => {
                    self.links.push((leader.link(), LinkRole::Leader, index));
                }
                Layer::Follower(follower) => {
                    self.links
                        .push((follower.link(), LinkRole::Follower, index));
                }
                Layer::Texture(texture) => self.textures.push((texture.texture_id(), index)),
                _ => {}
            }
            if layer_has_closure(layer) {
                self.closures.push(index);
            }
        }
    }

    fn clear(&mut self) {
        self.links.clear();
        self.textures.clear();
        self.closures.clear();
    }
}

/// What one boundary paints: one paint-order stream of effects, chunks and
/// child slots, built by a [`ContentBuilder`] so every entry's parent is an
/// earlier effect.
///
/// The stream and everything that depends only on it are shared; each
/// effect's layer is held beside it, so a composited-layer update
/// ([`SceneWriter::with_effects`](super::SceneWriter::with_effects)) copies
/// the layers and shares the stream.
#[derive(Debug)]
pub struct BoundaryContent {
    stream: Arc<Stream>,
    /// One layer per effect, in paint order.
    layers: Vec<Layer>,
    indexes: LayerIndexes,
}

impl BoundaryContent {
    /// Starts an empty stream.
    #[must_use]
    pub fn builder() -> ContentBuilder {
        ContentBuilder::default()
    }

    fn of(stream: Arc<Stream>, layers: Vec<Layer>) -> Self {
        let indexes = LayerIndexes::of(&stream.effects, &layers);
        Self {
            stream,
            layers,
            indexes,
        }
    }

    /// The stream in paint order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.stream.entries
    }

    /// The layer of the effect at `entry`, `None` when it names no effect.
    #[must_use]
    pub fn effect_layer(&self, entry: EntryIndex) -> Option<&Layer> {
        let at = self.effect_position(entry)?;
        self.layers.get(at)
    }

    /// Each effect's entry and layer, in paint order.
    pub fn effect_layers(&self) -> impl ExactSizeIterator<Item = (EntryIndex, &Layer)> + '_ {
        self.stream.effects.iter().copied().zip(self.layers.iter())
    }

    /// The child boundaries the stream names, in paint order.
    pub fn slots(&self) -> impl Iterator<Item = SceneNodeId> + '_ {
        self.stream
            .structural
            .iter()
            .filter_map(|index| match &self.stream.entries[index.get()] {
                Entry::Slot { node, .. } => Some(*node),
                Entry::Effect { .. } | Entry::Chunk { .. } => None,
            })
    }

    /// Where the effect at `entry` stands among the content's effects.
    pub(crate) fn effect_position(&self, entry: EntryIndex) -> Option<usize> {
        self.stream.effects.binary_search(&entry).ok()
    }

    /// The entry of each effect, ascending.
    pub(crate) fn effects(&self) -> &[EntryIndex] {
        &self.stream.effects
    }

    /// Each effect's layer, as [`Self::effects`] orders them.
    pub(crate) fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// The scope of the effect at `position` among the effects.
    pub(crate) fn effect_scope(&self, position: usize) -> EffectScope {
        self.stream.scopes[position]
    }

    /// Each effect and slot entry, ascending.
    pub(crate) fn structural(&self) -> &[EntryIndex] {
        &self.stream.structural
    }

    /// What the top-level chunks draw, in the boundary's space.
    pub(crate) fn top_chunks(&self) -> Reach {
        self.stream.top_chunks
    }

    /// Whether `other` holds this very stream, its layers aside.
    pub(crate) fn shares_stream(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stream, &other.stream)
    }

    pub(crate) fn links(&self) -> &[(LayerLink, LinkRole, EntryIndex)] {
        &self.indexes.links
    }

    /// Each entry that draws an external texture, by texture, in paint
    /// order.
    pub(crate) fn textures(&self) -> impl Iterator<Item = (TextureId, EntryIndex)> + '_ {
        merged(
            &self.stream.chunk_textures,
            &self.indexes.textures,
            |(_, entry)| *entry,
        )
        .copied()
    }

    /// The slot that names `child`, if one does.
    pub(crate) fn slot_entry(&self, child: NodeKey) -> Option<EntryIndex> {
        let slots = &self.stream.slot_entries;
        let at = slots
            .binary_search_by_key(&child.table_key(), |(node, _)| node.key().table_key())
            .ok()?;
        let (node, entry) = slots[at];
        (node.key() == child).then_some(entry)
    }

    /// Each entry that reads pixels other than those it draws, in paint
    /// order: a backdrop or image filter, a colour filter painting
    /// transparent pixels, a chunk that reads the destination. A node with
    /// one is filtered, and closing damage visits only these entries.
    pub(crate) fn closure_entries(&self) -> impl Iterator<Item = EntryIndex> + '_ {
        merged(
            &self.stream.chunk_closures,
            &self.indexes.closures,
            |entry| *entry,
        )
        .copied()
    }

    pub(crate) fn is_filtered(&self) -> bool {
        !self.stream.chunk_closures.is_empty() || !self.indexes.closures.is_empty()
    }

    /// Writes into `target` the same stream with the effects at the given
    /// entries replaced by new layers, the stream shared: how a
    /// composited-layer update republishes a boundary without repainting it
    /// ([`SceneWriter::with_effects`](super::SceneWriter::with_effects)). It
    /// costs the content's effects, not its chunks, and reuses the vectors
    /// `target` holds.
    ///
    /// # Errors
    ///
    /// The first index that names no effect, or names one a second time;
    /// `target` then holds every layer it was handed, placed or not,
    /// for the caller to drop where no borrow is held.
    pub(crate) fn effects_into(
        &self,
        target: &mut Self,
        replaced: impl IntoIterator<Item = (EntryIndex, Layer)>,
    ) -> Result<(), EntryIndex> {
        target.stream = Arc::clone(&self.stream);
        target.layers.clone_from(&self.layers);
        // Which positions this update wrote, so a repeated index is refused
        // instead of dropping the layer it would replace.
        let mut written: SmallVec<[u64; 4]> = SmallVec::new();
        written.resize(self.layers.len().div_ceil(64), 0);
        let mut refused = None;
        for (index, layer) in replaced {
            let position = self.effect_position(index).filter(|&at| {
                let (word, bit) = (at / 64, 1_u64 << (at % 64));
                let fresh = written[word] & bit == 0;
                written[word] |= bit;
                fresh
            });
            match (refused, position) {
                // The layer it replaces is a clone of one `self` still holds,
                // so its drop runs no annotation's destructor.
                (None, Some(at)) => target.layers[at] = layer,
                // A refused update keeps every layer it was handed, so their
                // destructors run where the caller drops `target`.
                (None, None) => {
                    refused = Some(index);
                    target.layers.push(layer);
                }
                (Some(_), _) => target.layers.push(layer),
            }
        }
        if let Some(index) = refused {
            return Err(index);
        }
        target.indexes.refill(&self.stream.effects, &target.layers);
        Ok(())
    }

    /// Layers held, one per effect.
    pub(crate) fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Layers the content's vector holds room for without allocating.
    pub(crate) fn layer_room(&self) -> usize {
        self.layers.capacity()
    }

    /// A content to write [`Self::effects_into`] into when no reclaimed one
    /// is held: `self`'s stream, room for its layers, none written yet.
    pub(crate) fn blank(&self) -> Self {
        Self {
            stream: Arc::clone(&self.stream),
            layers: Vec::with_capacity(self.layers.len()),
            indexes: LayerIndexes::default(),
        }
    }

    /// Moves into `into`, emptied, the chunks of a stream nothing else holds
    /// whose commands nothing else holds and hold room for at most
    /// `kept_room` commands, while `into` holds fewer than `cap`. It takes the
    /// stream's entries apart, so it runs just before [`Self::scrub`] or the
    /// content's drop, with no borrow held: the commands drop here.
    pub(crate) fn harvest_chunks(
        &mut self,
        into: &mut Vec<Arc<Chunk>>,
        cap: usize,
        kept_room: usize,
    ) {
        let Some(stream) = Arc::get_mut(&mut self.stream) else {
            return;
        };
        for entry in stream.entries.drain(..) {
            let Entry::Chunk { mut chunk, .. } = entry else {
                continue;
            };
            if into.len() >= cap {
                continue;
            }
            let kept = Arc::get_mut(&mut chunk)
                .is_some_and(|held| held.room() <= kept_room && held.empty());
            if kept {
                into.push(chunk);
            }
        }
    }

    /// Lets go of everything the content holds, keeping its vectors' room
    /// and taking `empty`'s stream: what is left holds no value of the scene.
    /// The layers and stream it held drop here, an annotation's value among
    /// them, so a caller runs this with no borrow held.
    pub(crate) fn scrub(&mut self, empty: &Self) {
        self.stream = Arc::clone(&empty.stream);
        self.layers.clear();
        self.indexes.clear();
    }

    /// Whether `other` puts the same entries into the three indexes.
    pub(crate) fn same_indexes(&self, other: &Self) -> bool {
        // Chunk and effect textures are compared apart: equal halves are
        // equal lists, and halves that differ only reindex.
        (self.shares_stream(other) || self.stream.chunk_textures == other.stream.chunk_textures)
            && self.indexes.links == other.indexes.links
            && self.indexes.textures == other.indexes.textures
            && self.is_filtered() == other.is_filtered()
    }
}

/// Two lists, each ascending by `key`, as one: no key is in both.
fn merged<'a, T>(
    first: &'a [T],
    second: &'a [T],
    key: impl Fn(&T) -> EntryIndex,
) -> impl Iterator<Item = &'a T> {
    let (mut a, mut b) = (first.iter().peekable(), second.iter().peekable());
    std::iter::from_fn(move || match (a.peek(), b.peek()) {
        (Some(x), Some(y)) => {
            if key(x) <= key(y) {
                a.next()
            } else {
                b.next()
            }
        }
        (Some(_), None) => a.next(),
        (None, _) => b.next(),
    })
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
    layers: Vec<Layer>,
    open: SmallVec<[EntryIndex; 8]>,
}

impl ContentBuilder {
    /// Pushes `layer` under the open effect and opens it.
    pub fn push_effect(&mut self, layer: Layer, owner: RenderId, ordinal: u32) -> EntryIndex {
        let index = self.push(|parent| Entry::Effect {
            owner,
            ordinal,
            parent,
        });
        self.layers.push(layer);
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
        BoundaryContent::of(Arc::new(Stream::of(self.entries)), self.layers)
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

/// Whether an effect of `layer` reads pixels other than those it draws, so
/// damage that meets it takes its read closure (ADR-0304 §6).
fn layer_has_closure(layer: &Layer) -> bool {
    match layer {
        Layer::BackdropFilter(_) | Layer::ImageFilter(_) => true,
        Layer::ColorFilter(filter) => filter.color_filter().modifies_transparent_black(),
        _ => false,
    }
}
