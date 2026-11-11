//! Where a version's content lands: the chain of spaces from the surface
//! down to an entry, the footprint of an entry's scope and of a subtree, and
//! the transform a follower resolves to (ADR-0304 §3, §4).
//!
//! Scene space is the surface's device pixels. Footprints only over-report:
//! a filter spreads by its reach under the transform relative to the
//! boundary that holds it, and the read closure (§6), which runs with each
//! filter's surface transform, takes whatever more it reaches.

use flui_foundation::geometry::{Matrix4, Offset, Rect};
use flui_painting::DamageExtent;
use flui_painting::paint::ImageFilter;
use smallvec::SmallVec;

use super::content::{BoundaryContent, Bounds, Entry, EntryIndex};
use super::id::NodeKey;
use super::version::{Address, NodeRecord, VersionInner};
use crate::damage::closure::footprint;
use crate::layer::Layer;
use crate::link::{MAX_FOLLOWER_NESTING, invert};

/// One step from a space into the one it holds, as a footprint crosses it
/// outward.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Link<'v> {
    /// The inner space's map into the outer one.
    Matrix(Matrix4),
    /// Nothing inside lands outside this rect of the inner space.
    Clip(Rect<f64>),
    /// What is inside spreads or moves as the filter sends it.
    Filter(&'v ImageFilter),
    /// The effect itself changes this rect of the inner space.
    Extent(Rect<f64>),
    /// The effect itself changes every pixel the clips outside it allow.
    Everywhere,
    /// Nothing inside is drawn: a follower that resolves to nothing.
    Hidden,
}

pub(crate) type Links<'v> = SmallVec<[Link<'v>; 16]>;

/// Where something lands, in one space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Reach {
    /// Nowhere.
    Nothing,
    /// Inside the rect.
    Rect(Rect<f64>),
    /// Anywhere.
    Unbounded,
    /// Somewhere a chain or a bound that is not finite cannot say.
    NonFinite,
}

impl Reach {
    pub(crate) fn of(bounds: Bounds) -> Self {
        match bounds {
            Bounds::Finite(rect) => Self::rect(rect),
            Bounds::Unbounded => Self::Unbounded,
        }
    }

    fn rect(rect: Rect<f64>) -> Self {
        if !rect.is_finite() {
            Self::NonFinite
        } else if rect.width() > 0.0 && rect.height() > 0.0 {
            Self::Rect(rect)
        } else {
            Self::Nothing
        }
    }

    pub(crate) fn union(self, other: Self) -> Self {
        match (self, other) {
            (Self::NonFinite, _) | (_, Self::NonFinite) => Self::NonFinite,
            (Self::Unbounded, _) | (_, Self::Unbounded) => Self::Unbounded,
            (Self::Nothing, reach) | (reach, Self::Nothing) => reach,
            (Self::Rect(a), Self::Rect(b)) => Self::Rect(a.union(&b)),
        }
    }

    /// Whether `self` covers `other`.
    pub(crate) fn contains(self, other: Self) -> bool {
        match (self, other) {
            (_, Self::Nothing) | (Self::Unbounded | Self::NonFinite, _) => true,
            (Self::Rect(a), Self::Rect(b)) => a.contains_rect(&b),
            _ => false,
        }
    }

    /// As stored bounds: an empty reach is an empty rect, and one that is not
    /// finite has no bound.
    pub(crate) fn bounds(self) -> Bounds {
        match self {
            Self::Nothing => Bounds::Finite(Rect::ZERO),
            Self::Rect(rect) => Bounds::Finite(rect),
            Self::Unbounded | Self::NonFinite => Bounds::Unbounded,
        }
    }
}

/// `rect` mapped by `matrix`, covered by a rect.
fn mapped(matrix: &Matrix4, rect: Rect<f64>) -> Reach {
    if !matrix.m.iter().all(|value| value.is_finite()) {
        return Reach::NonFinite;
    }
    match DamageExtent::rect(rect).transformed(matrix).covering_rect() {
        Some(rect) => Reach::rect(rect),
        // A projective map past the camera has no bound.
        None => Reach::Unbounded,
    }
}

/// `reach`, in the innermost space of `links` (listed outermost first),
/// mapped out through each of them into the outermost.
pub(crate) fn map_out(links: &[Link<'_>], reach: Reach) -> Reach {
    // The map from each step's space into the outermost.
    let mut ctms: SmallVec<[Matrix4; 16]> = SmallVec::with_capacity(links.len());
    let mut ctm = Matrix4::IDENTITY;
    for link in links {
        if let Link::Matrix(matrix) = link {
            ctm *= *matrix;
        }
        ctms.push(ctm);
    }
    let mut out = match reach {
        Reach::Rect(rect) => mapped(&ctm, rect),
        other => other,
    };
    for (link, ctm) in links.iter().zip(&ctms).rev() {
        out = match *link {
            Link::Matrix(_) => out,
            Link::Hidden => Reach::Nothing,
            Link::Clip(clip) => match (out, mapped(ctm, clip)) {
                (Reach::Rect(inside), Reach::Rect(clip)) => {
                    inside.intersect(&clip).map_or(Reach::Nothing, Reach::rect)
                }
                (Reach::Unbounded, clip @ (Reach::Rect(_) | Reach::Nothing)) => clip,
                (Reach::Rect(_), Reach::Nothing) => Reach::Nothing,
                (out, _) => out,
            },
            Link::Filter(filter) => match out {
                Reach::Rect(input) => {
                    footprint(filter, input, ctm).map_or(Reach::Unbounded, Reach::rect)
                }
                other => other,
            },
            Link::Extent(rect) => out.union(mapped(ctm, rect)),
            Link::Everywhere => Reach::Unbounded,
        };
    }
    out
}

/// Why a follower draws nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unresolved {
    /// Unlinked and hidden, or a singular or non-finite chain.
    Hidden,
    /// Followers nest past [`MAX_FOLLOWER_NESTING`], or cycle through their
    /// leaders.
    TooDeep,
}

/// The node is not under the root, so nothing of it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unattached;

/// Reads one version's geometry.
#[derive(Clone, Copy)]
pub(crate) struct Geometry<'v> {
    version: &'v VersionInner,
}

impl<'v> Geometry<'v> {
    pub(crate) fn new(version: &'v VersionInner) -> Self {
        Self { version }
    }

    fn record(self, key: NodeKey) -> &'v NodeRecord {
        self.version
            .record(key)
            .expect("BUG: geometry reads nodes of its own version")
    }

    /// Whether the root reaches `key`.
    pub(crate) fn is_attached(self, key: NodeKey) -> bool {
        let mut at = key;
        loop {
            match self.version.record(at).and_then(NodeRecord::parent) {
                Some(parent) => at = parent,
                None => return at == self.version.root && self.version.record(at).is_some(),
            }
        }
    }

    /// The links from the surface into `key`'s content space.
    pub(crate) fn node_links(
        self,
        key: NodeKey,
        out: &mut Links<'v>,
        nesting: usize,
    ) -> Result<(), Unattached> {
        let mut path: SmallVec<[NodeKey; 16]> = SmallVec::new();
        let mut at = key;
        path.push(at);
        while let Some(parent) = self.version.record(at).and_then(NodeRecord::parent) {
            path.push(parent);
            at = parent;
        }
        if at != self.version.root || self.version.record(at).is_none() {
            return Err(Unattached);
        }
        for pair in path.windows(2).rev() {
            let (child, parent) = (pair[0], pair[1]);
            let slot = self
                .record(parent)
                .content()
                .slot_entry(child)
                .expect("BUG: a node's parent names it in a slot");
            self.content_links(parent, slot, out, nesting);
            out.push(Link::Matrix(placement(self.record(child))));
        }
        Ok(())
    }

    /// The links of the effects above `entry` in `key`'s content, outermost
    /// first.
    pub(crate) fn content_links(
        self,
        key: NodeKey,
        entry: EntryIndex,
        out: &mut Links<'v>,
        nesting: usize,
    ) {
        let content = self.record(key).content();
        let mut above: SmallVec<[EntryIndex; 8]> = SmallVec::new();
        let mut at = content.entries()[entry.get()].parent();
        while let Some(effect) = at {
            above.push(effect);
            at = content.entries()[effect.get()].parent();
        }
        for &effect in above.iter().rev() {
            if let Entry::Effect { layer, .. } = &content.entries()[effect.get()] {
                self.effect_links(
                    layer,
                    Address {
                        node: key,
                        entry: effect,
                    },
                    out,
                    nesting,
                );
            }
        }
    }

    /// The links `layer`, at `at`, puts between its parent's space and its
    /// children's: its map first, then what it does inside it.
    pub(crate) fn effect_links(
        self,
        layer: &'v Layer,
        at: Address,
        out: &mut Links<'v>,
        nesting: usize,
    ) {
        match layer {
            Layer::Transform(transform) => out.push(Link::Matrix(*transform.transform())),
            Layer::Follower(_) => out.push(match self.resolve(at, nesting + 1) {
                Ok(transform) => Link::Matrix(transform),
                Err(Unresolved::Hidden | Unresolved::TooDeep) => Link::Hidden,
            }),
            _ => {
                let offset = layer.local_translation();
                if offset != Offset::ZERO {
                    out.push(Link::Matrix(Matrix4::translation(
                        offset.dx, offset.dy, 0.0,
                    )));
                }
            }
        }
        match layer {
            Layer::ClipRect(clip) if clip.clips() => out.push(Link::Clip(clip.bounds())),
            Layer::ClipRRect(clip) if clip.clips() => out.push(Link::Clip(clip.bounds())),
            Layer::ClipPath(clip) if clip.clips() => out.push(Link::Clip(clip.bounds())),
            Layer::ClipSuperellipse(clip) if clip.clips() => out.push(Link::Clip(clip.bounds())),
            Layer::ImageFilter(filter) => {
                out.push(Link::Filter(filter.filter()));
                if filter.filter().modifies_transparent_black() {
                    out.push(Link::Everywhere);
                }
            }
            Layer::ColorFilter(filter) if filter.color_filter().modifies_transparent_black() => {
                out.push(Link::Everywhere);
            }
            // Its composite changes pixels its children never inked.
            Layer::Opacity(opacity)
                if !opacity.blend().keeps_destination_under_transparent_source() =>
            {
                out.push(Link::Everywhere);
            }
            Layer::ShaderMask(_)
            | Layer::BackdropFilter(_)
            | Layer::Texture(_)
            | Layer::PlatformView(_)
            | Layer::PerformanceOverlay(_)
            | Layer::Picture(_)
            | Layer::Canvas(_) => match layer.bounds() {
                Some(bounds) => out.push(Link::Extent(bounds)),
                None => out.push(Link::Everywhere),
            },
            _ => {}
        }
    }

    /// Where `entry` of `key` and everything in its scope land, in the space
    /// its parent effect gives (`key`'s own space for a top-level entry).
    pub(crate) fn scope_local(self, key: NodeKey, entry: EntryIndex, nesting: usize) -> Reach {
        let content = self.record(key).content();
        match &content.entries()[entry.get()] {
            Entry::Chunk { chunk, .. } => Reach::of(chunk.bounds()),
            Entry::Slot { node, .. } => {
                let child = self.record(node.key());
                map_out(
                    &[Link::Matrix(placement(child))],
                    Reach::of(child.subtree()),
                )
            }
            Entry::Effect { layer, .. } => {
                let mut links = Links::new();
                self.effect_links(layer, Address { node: key, entry }, &mut links, nesting);
                let mut inside = Reach::Nothing;
                for child in children(content, entry) {
                    inside = inside.union(self.scope_local(key, child, nesting));
                }
                map_out(&links, inside)
            }
        }
    }

    /// What `key`'s content and subtree change, in its own space, from its
    /// children's stored bounds.
    pub(crate) fn subtree_local(self, key: NodeKey) -> Reach {
        let content = self.record(key).content();
        let mut reach = Reach::Nothing;
        for (position, entry) in content.entries().iter().enumerate() {
            if entry.parent().is_none() {
                reach = reach.union(self.scope_local(key, EntryIndex::at(position), 0));
            }
        }
        reach
    }

    /// Where `entry` of `key` and its scope land on the surface; nothing when
    /// the node is not attached.
    pub(crate) fn entry_reach(self, key: NodeKey, entry: EntryIndex) -> Reach {
        let mut links = Links::new();
        if self.node_links(key, &mut links, 0).is_err() {
            return Reach::Nothing;
        }
        self.content_links(key, entry, &mut links, 0);
        map_out(&links, self.scope_local(key, entry, 0))
    }

    /// Where `key`'s subtree lands on the surface, from its stored bounds;
    /// nothing when it is not attached.
    pub(crate) fn subtree_reach(self, key: NodeKey) -> Reach {
        let mut links = Links::new();
        if self.node_links(key, &mut links, 0).is_err() {
            return Reach::Nothing;
        }
        map_out(&links, Reach::of(self.record(key).subtree()))
    }

    /// The map from the space under the effect at `at` (its own map applied)
    /// to the surface; `None` when nothing there is drawn.
    pub(crate) fn effect_ctm(self, at: Address) -> Option<Matrix4> {
        let mut links = Links::new();
        self.node_links(at.node, &mut links, 0).ok()?;
        self.content_links(at.node, at.entry, &mut links, 0);
        if let Entry::Effect { layer, .. } =
            &self.record(at.node).content().entries()[at.entry.get()]
        {
            self.effect_links(layer, at, &mut links, 0);
        }
        product(&links).ok()
    }

    /// What the children of the effect at `at` draw, on the surface and
    /// unclipped: the input a foreground filter there takes.
    pub(crate) fn filter_input(self, at: Address) -> Reach {
        let Some(ctm) = self.effect_ctm(at) else {
            return Reach::Nothing;
        };
        let content = self.record(at.node).content();
        let mut inside = Reach::Nothing;
        for child in children(content, at.entry) {
            inside = inside.union(self.scope_local(at.node, child, 0));
        }
        match inside {
            Reach::Rect(rect) => mapped(&ctm, rect),
            other => other,
        }
    }

    /// The transform the follower at `at` applies to its children so their
    /// content lands on its leader: `F⁻¹ · L · translate(linked_offset)`,
    /// with `F` the surface map of the follower's parent space and `L` the
    /// leader's, its own offset applied. This is `resolve_follower_transform`'s
    /// rule (ADR-0276) with both chains taken from the surface; the part they
    /// share cancels. A leader clip that hides the follower is not consulted:
    /// a footprint that shows a hidden follower over-reports.
    pub(crate) fn resolve(self, at: Address, nesting: usize) -> Result<Matrix4, Unresolved> {
        if nesting > MAX_FOLLOWER_NESTING {
            return Err(Unresolved::TooDeep);
        }
        let Entry::Effect {
            layer: Layer::Follower(follower),
            ..
        } = &self.record(at.node).content().entries()[at.entry.get()]
        else {
            return Err(Unresolved::Hidden);
        };
        let unlinked = || {
            if follower.show_when_unlinked() {
                let offset = follower.target_offset();
                finite(Matrix4::translation(offset.dx, offset.dy, 0.0))
            } else {
                Err(Unresolved::Hidden)
            }
        };
        let Some(leader_at) = self
            .version
            .links
            .get(follower.link().id())
            .and_then(|users| users.leaders.last().copied())
        else {
            return unlinked();
        };
        let Entry::Effect {
            layer: leader_layer @ Layer::Leader(leader),
            ..
        } = &self.record(leader_at.node).content().entries()[leader_at.entry.get()]
        else {
            return unlinked();
        };
        if self.is_inside(leader_at, at) {
            tracing::warn!(
                "a leader is inside its follower's subtree (a leader must precede its \
                 follower in paint order); using the unlinked contract",
            );
            return unlinked();
        }
        let mut leader_links = Links::new();
        if self
            .node_links(leader_at.node, &mut leader_links, nesting)
            .is_err()
        {
            return unlinked();
        }
        self.content_links(leader_at.node, leader_at.entry, &mut leader_links, nesting);
        self.effect_links(leader_layer, leader_at, &mut leader_links, nesting);
        let mut follower_links = Links::new();
        self.node_links(at.node, &mut follower_links, nesting)
            .map_err(|Unattached| Unresolved::Hidden)?;
        self.content_links(at.node, at.entry, &mut follower_links, nesting);
        let leader_ctm = product(&leader_links)?;
        let follower_ctm = product(&follower_links)?;
        let linked = follower.linked_offset(leader.size());
        let transform = invert(&follower_ctm).ok_or(Unresolved::Hidden)?
            * leader_ctm
            * Matrix4::translation(linked.dx, linked.dy, 0.0);
        invert(&transform).ok_or(Unresolved::Hidden)?;
        finite(transform)
    }

    /// Whether the entry at `inner` lies in the scope of the effect at
    /// `outer`, in its own content or in a boundary a slot of that scope
    /// names.
    fn is_inside(self, inner: Address, outer: Address) -> bool {
        let (mut node, mut entry) = (inner.node, inner.entry);
        loop {
            if node == outer.node {
                return in_scope(self.record(node).content(), entry, outer.entry);
            }
            let Some(parent) = self.version.record(node).and_then(NodeRecord::parent) else {
                return false;
            };
            entry = self
                .record(parent)
                .content()
                .slot_entry(node)
                .expect("BUG: a node's parent names it in a slot");
            node = parent;
        }
    }
}

/// The product of every map in `links`, outermost first.
fn product(links: &[Link<'_>]) -> Result<Matrix4, Unresolved> {
    let mut ctm = Matrix4::IDENTITY;
    for link in links {
        match link {
            Link::Matrix(matrix) => ctm *= *matrix,
            Link::Hidden => return Err(Unresolved::Hidden),
            Link::Clip(_) | Link::Filter(_) | Link::Extent(_) | Link::Everywhere => {}
        }
    }
    finite(ctm)
}

fn finite(transform: Matrix4) -> Result<Matrix4, Unresolved> {
    if transform.m.iter().all(|value| value.is_finite()) {
        Ok(transform)
    } else {
        Err(Unresolved::Hidden)
    }
}

fn placement(record: &NodeRecord) -> Matrix4 {
    let offset = record.placement().offset();
    Matrix4::translation(offset.dx, offset.dy, 0.0)
}

/// The entries whose parent is `effect`: they follow it, before the first
/// entry outside its scope.
fn children(
    content: &BoundaryContent,
    effect: EntryIndex,
) -> impl Iterator<Item = EntryIndex> + '_ {
    content.entries()[effect.get() + 1..]
        .iter()
        .enumerate()
        .take_while(move |(offset, _)| {
            in_scope(content, EntryIndex::at(effect.get() + 1 + offset), effect)
        })
        .filter(move |(_, entry)| entry.parent() == Some(effect))
        .map(move |(offset, _)| EntryIndex::at(effect.get() + 1 + offset))
}

/// Whether `entry` sits under `effect` in `content`.
fn in_scope(content: &BoundaryContent, entry: EntryIndex, effect: EntryIndex) -> bool {
    let mut at = content.entries()[entry.get()].parent();
    while let Some(parent) = at {
        if parent == effect {
            return true;
        }
        at = content.entries()[parent.get()].parent();
    }
    false
}
