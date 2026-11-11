//! Where a version's content lands: the chain of spaces from the surface
//! down to an entry, the footprint of an entry's scope and of a subtree, and
//! the transform a follower resolves to (ADR-0304 §3, §4).
//!
//! Scene space is the surface's device pixels. Footprints only over-report:
//! a filter spreads by its reach under the transform relative to the
//! boundary that holds it, and by its renderer kernel in device pixels, which
//! a scope and a subtree carry beside their local bounds (`Scope`) and
//! add once mapped to the surface, where no ancestor scales it.

use std::cell::Cell;

use flui_foundation::geometry::{Matrix4, Offset, Point, Rect};
use flui_painting::DamageExtent;
use flui_painting::paint::ImageFilter;
use smallvec::SmallVec;

use super::content::{BoundaryContent, Bounds, Entry, EntryIndex, LinkRole};
use super::id::NodeKey;
use super::version::{Address, NodeRecord, VersionInner};
use crate::damage::closure::{device_spread, footprint, moved};
use crate::layer::Layer;
use crate::link::{MAX_FOLLOWER_NESTING, invert};
use crate::tree::annotations::clip_admits;
use crate::{LayerLink, LeaderClip};

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

/// What a scope draws, in the space that holds it, and how far its filters
/// spread that in device pixels once mapped to the surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Scope {
    pub(crate) local: Reach,
    pub(crate) kernel: f64,
}

impl Scope {
    const EMPTY: Self = Self {
        local: Reach::Nothing,
        kernel: 0.0,
    };

    /// Both scopes, side by side: the union of what they draw and the
    /// larger kernel.
    fn join(self, other: Self) -> Self {
        Self {
            local: self.local.union(other.local),
            kernel: self.kernel.max(other.kernel),
        }
    }

    /// The scope as the space `links` (outermost first) lead out to holds
    /// it, one step at a time from the innermost. A filter moves what it
    /// takes and adds its spread to the kernel (`device_spread`); a clip
    /// over a scope with a kernel bounds it by the clip, its kernel spent,
    /// since a local bound cannot hold device pixels but the clip still
    /// bounds all they draw.
    pub(crate) fn through(self, links: &[Link<'_>]) -> Self {
        links.iter().rev().fold(self, |scope, link| match *link {
            Link::Matrix(matrix) => Self {
                local: match scope.local {
                    Reach::Rect(rect) => mapped(&matrix, rect),
                    other => other,
                },
                ..scope
            },
            Link::Hidden => Self::EMPTY,
            Link::Clip(clip) if scope.kernel > 0.0 && scope.local != Reach::Nothing => Self {
                local: Reach::rect(clip),
                kernel: 0.0,
            },
            Link::Clip(clip) => Self {
                local: map_out(&[Link::Clip(clip)], scope.local),
                ..scope
            },
            Link::Filter(filter) => Self {
                local: map_out(&[Link::Filter(filter)], scope.local),
                kernel: device_spread(filter, scope.kernel),
            },
            Link::Extent(rect) => Self {
                local: scope.local.union(Reach::rect(rect)),
                ..scope
            },
            Link::Everywhere => Self {
                local: Reach::Unbounded,
                ..scope
            },
        })
    }
}

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

    /// `self` grown by `kernel` device pixels, a reach already mapped to
    /// the surface: an infinite kernel has no bound.
    pub(crate) fn spread(self, kernel: f64) -> Self {
        match self {
            Self::Rect(_) if kernel == 0.0 => self,
            Self::Rect(_) if !kernel.is_finite() => Self::Unbounded,
            Self::Rect(rect) => Self::rect(rect.expand(kernel)),
            other => other,
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
    // Most steps of a chain are placements and offsets: a translation maps a
    // rect to the same rect moved, which is what the general map computes
    // for it, corner for corner.
    if let Some((dx, dy)) = matrix.as_translation() {
        return Reach::rect(Rect::from_ltrb(
            rect.min.x + dx,
            rect.min.y + dy,
            rect.max.x + dx,
            rect.max.y + dy,
        ));
    }
    match DamageExtent::rect(rect).transformed(matrix).covering_rect() {
        Some(rect) => Reach::rect(rect),
        // A projective map past the camera has no bound.
        None => Reach::Unbounded,
    }
}
/// `reach`, in the innermost space of `links` (listed outermost first),
/// mapped out through each of them into the outermost space of one
/// boundary's content. A filter on the way moves what it takes and does not
/// spread it: its spread is in device pixels, which the scope's kernel
/// carries to the surface.
pub(crate) fn map_out(links: &[Link<'_>], reach: Reach) -> Reach {
    map_through(links, reach, 0.0, Spread::Kernel)
}

/// How a filter on a chain grows what it takes.
#[derive(Clone, Copy)]
enum Spread {
    /// By its reach under the chain's map: the chain ends on the surface.
    Reach,
    /// Not at all: a kernel carries it.
    Kernel,
}

/// `map_out`, with `reach` grown by `kernel` device pixels once mapped and
/// before the links' clips and filters: the device-space spread of the
/// filters inside it, which bounds in a local space cannot hold.
pub(crate) fn map_out_spread(links: &[Link<'_>], reach: Reach, kernel: f64) -> Reach {
    map_through(links, reach, kernel, Spread::Reach)
}

fn map_through(links: &[Link<'_>], reach: Reach, kernel: f64, spread: Spread) -> Reach {
    // The map from the space of each step that reads it (a clip, a filter,
    // an extent) into the outermost, in chain order.
    let mut ctms: SmallVec<[Matrix4; 4]> = SmallVec::new();
    let mut ctm = Matrix4::IDENTITY;
    for link in links {
        match link {
            Link::Matrix(matrix) => ctm *= *matrix,
            Link::Clip(_) | Link::Filter(_) | Link::Extent(_) => ctms.push(ctm),
            Link::Hidden | Link::Everywhere => {}
        }
    }
    let mut out = match reach {
        Reach::Rect(rect) => mapped(&ctm, rect).spread(kernel),
        other => other,
    };
    for link in links.iter().rev() {
        out = match *link {
            Link::Matrix(_) => out,
            Link::Hidden => Reach::Nothing,
            Link::Everywhere => Reach::Unbounded,
            Link::Clip(_) | Link::Filter(_) | Link::Extent(_) => {
                let ctm = ctms
                    .pop()
                    .expect("BUG: every clip, filter and extent of the chain has its map");
                match *link {
                    Link::Clip(clip) => match (out, mapped(&ctm, clip)) {
                        (Reach::Rect(inside), Reach::Rect(clip)) => {
                            inside.intersect(&clip).map_or(Reach::Nothing, Reach::rect)
                        }
                        (Reach::Unbounded, clip @ (Reach::Rect(_) | Reach::Nothing)) => clip,
                        (Reach::Rect(_), Reach::Nothing) => Reach::Nothing,
                        (out, _) => out,
                    },
                    Link::Filter(filter) => match out {
                        Reach::Rect(input) => match spread {
                            Spread::Reach => footprint(filter, input, &ctm),
                            Spread::Kernel => moved(filter, input, &ctm),
                        }
                        .map_or(Reach::Unbounded, Reach::rect),
                        other => other,
                    },
                    Link::Extent(rect) => out.union(mapped(&ctm, rect)),
                    Link::Matrix(_) | Link::Hidden | Link::Everywhere => out,
                }
            }
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

/// One step from an entry toward the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Up {
    /// The effect at this address holds the entry below it.
    Effect(Address),
    /// The node's placement maps its content into its parent's.
    Place(NodeKey),
}

/// The node is not under the root, so nothing of it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Unattached;

/// What the follower resolutions of a patch's footprint report.
pub(crate) struct Watch<'v> {
    /// Every resolution, nested ones included.
    pub(crate) resolutions: Cell<u64>,
    /// Whether the patch touched or placed a node.
    pub(crate) changed: &'v dyn Fn(NodeKey) -> bool,
    /// Whether a resolution met a follower under [`LeaderClip::Hide`] with a
    /// changed node on its leader's branch: a clip there can show or hide it,
    /// and with it every follower whose chain passes through it, with no
    /// transform changed.
    pub(crate) hiding_changed: Cell<bool>,
}

/// Reads one version's geometry.
#[derive(Clone, Copy)]
pub(crate) struct Geometry<'v> {
    version: &'v VersionInner,
    /// What the resolutions it makes report, when a patch is being read.
    watch: Option<&'v Watch<'v>>,
    /// Whether a follower under [`LeaderClip::Hide`] whose leader's anchor a
    /// clip on the leader's chain leaves out resolves to nothing, as the
    /// renderer draws it. A footprint reader leaves it shown and over-reports.
    clip_hides: bool,
}

impl<'v> Geometry<'v> {
    pub(crate) fn new(version: &'v VersionInner) -> Self {
        Self {
            version,
            watch: None,
            clip_hides: false,
        }
    }

    /// A reader that resolves followers exactly as the renderer draws them,
    /// a leader clip that hides one included.
    pub(crate) fn shown(version: &'v VersionInner) -> Self {
        Self {
            version,
            watch: None,
            clip_hides: true,
        }
    }

    /// A reader whose follower resolutions report to `watch`.
    pub(crate) fn watching(version: &'v VersionInner, watch: &'v Watch<'v>) -> Self {
        Self {
            version,
            clip_hides: false,
            watch: Some(watch),
        }
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

    /// The leader a follower of `link` resolves against: the last attached
    /// one in the index. A published node no slot reaches draws nothing, so a
    /// leader in it never shadows the leader the scene draws, as
    /// [`SceneVersion::flatten`](super::SceneVersion::flatten) leaves it out.
    pub(crate) fn leader(self, link: LayerLink) -> Option<Address> {
        self.version
            .links
            .get(link.id())?
            .leaders
            .iter()
            .rev()
            .copied()
            .find(|leader| self.is_attached(leader.node))
    }

    /// `key` and every node above it.
    pub(crate) fn path_up(self, key: NodeKey) -> impl Iterator<Item = NodeKey> + 'v {
        let version = self.version;
        std::iter::successors(Some(key), move |&at| {
            version.record(at).and_then(NodeRecord::parent)
        })
    }

    /// The links from the surface into `key`'s content space: the root's
    /// placement, then each slot's effects and placement down to `key`.
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
        // The root's placement offsets the whole scene, as `flatten` draws it.
        out.push(Link::Matrix(placement(self.record(at))));
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
            if let Some(layer) = content.effect_layer(effect) {
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
            Layer::Follower(_) => out.push(match self.resolve(at, nesting) {
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
            | Layer::PerformanceOverlay(_) => match layer.bounds() {
                Some(bounds) => out.push(Link::Extent(bounds)),
                None => out.push(Link::Everywhere),
            },
            // A recording changes what its damage extent covers, which takes
            // full-canvas fills, strokes, shadows and glyph overflow that its
            // bounds leave out, as a chunk's bounds do.
            Layer::Picture(picture) => out.push(extent_link(picture.picture().damage_extent())),
            Layer::Canvas(canvas) => out.push(extent_link(canvas.display_list().damage_extent())),
            _ => {}
        }
    }

    /// What the scope of `start` (an entry and, for an effect, every entry
    /// under it; `None` for the whole content) of `key` draws in the space
    /// that holds it (`key`'s own space for a top-level entry), and how far,
    /// in device pixels, the filters among them spread it once mapped to the
    /// surface (`device_spread`), slotted subtrees included.
    ///
    /// Only the scope's effects and slots are folded: each effect's inside
    /// starts from the union of its direct chunks, and the whole content's
    /// from that of its top-level chunks, which the content holds. One pass
    /// from the last of them back folds each into its parent's, so a chain of
    /// effects of any depth costs no stack.
    fn scope(
        self,
        key: NodeKey,
        start: Option<EntryIndex>,
        filters: Option<&mut Vec<(EntryIndex, Scope)>>,
    ) -> Scope {
        let content = self.record(key).content();
        let entries = content.entries();
        let all = content.structural();
        let (structural, first_effect, whole, floor) = match start {
            None => (
                all,
                0,
                Scope {
                    local: content.top_chunks(),
                    kernel: 0.0,
                },
                0,
            ),
            Some(entry) => match &entries[entry.get()] {
                Entry::Chunk { chunk, .. } => {
                    return Scope {
                        local: Reach::of(chunk.bounds()),
                        kernel: 0.0,
                    };
                }
                Entry::Slot { node, .. } => return self.slot_scope(node.key()),
                Entry::Effect { .. } => {
                    let position = content
                        .effect_position(entry)
                        .expect("BUG: an effect entry is among its content's effects");
                    let end = content.effect_scope(position).end;
                    let from = all.partition_point(|index| *index < entry);
                    let to = all.partition_point(|index| index.get() < end);
                    (&all[from..to], position, Scope::EMPTY, entry.get())
                }
            },
        };
        self.fold(key, structural, first_effect, whole, floor, filters)
    }

    /// What the effect `entry` of `key` holds, in the space under its own
    /// map: its direct chunks and every entry under it. The scope of the
    /// effect is this taken through the effect's own links, so two versions
    /// whose effect differs and whose inside does not fold it once.
    pub(crate) fn effect_inside(self, key: NodeKey, entry: EntryIndex) -> Scope {
        let content = self.record(key).content();
        let position = content
            .effect_position(entry)
            .expect("BUG: an effect entry is among its content's effects");
        let scope = content.effect_scope(position);
        let all = content.structural();
        let from = all.partition_point(|index| *index <= entry);
        let to = all.partition_point(|index| index.get() < scope.end);
        self.fold(
            key,
            &all[from..to],
            position + 1,
            Scope {
                local: scope.chunks,
                kernel: 0.0,
            },
            entry.get() + 1,
            None,
        )
    }

    /// Folds `structural`, the effects and slots of one scope, into `whole`:
    /// an entry whose parent is at or after `floor` joins its parent's
    /// inside, any other joins `whole`. `first_effect` is the position of the
    /// first effect among them.
    fn fold(
        self,
        key: NodeKey,
        structural: &[EntryIndex],
        first_effect: usize,
        mut whole: Scope,
        floor: usize,
        mut filters: Option<&mut Vec<(EntryIndex, Scope)>>,
    ) -> Scope {
        let content = self.record(key).content();
        let entries = content.entries();
        // Each structural entry's inside, and an effect's position among the
        // content's effects: those of the scope are consecutive.
        let mut inside: SmallVec<[Scope; 16]> = SmallVec::with_capacity(structural.len());
        let mut positions: SmallVec<[usize; 16]> = SmallVec::with_capacity(structural.len());
        let mut next_effect = first_effect;
        for index in structural {
            match entries[index.get()] {
                Entry::Effect { .. } => {
                    inside.push(Scope {
                        local: content.effect_scope(next_effect).chunks,
                        kernel: 0.0,
                    });
                    positions.push(next_effect);
                    next_effect += 1;
                }
                Entry::Slot { .. } | Entry::Chunk { .. } => {
                    inside.push(Scope::EMPTY);
                    positions.push(usize::MAX);
                }
            }
        }
        for (offset, &index) in structural.iter().enumerate().rev() {
            let entry = &entries[index.get()];
            let scope = match entry {
                Entry::Slot { node, .. } => self.slot_scope(node.key()),
                Entry::Effect { .. } => {
                    let held = inside[offset];
                    let layer = &content.layers()[positions[offset]];
                    let mut links = Links::new();
                    let at = Address {
                        node: key,
                        entry: index,
                    };
                    self.effect_links(layer, at, &mut links, 0);
                    if let Layer::ImageFilter(_) = layer
                        && let Some(filters) = filters.as_deref_mut()
                    {
                        filters.push((at.entry, held));
                    }
                    held.through(&links)
                }
                // The structural entries hold no chunk.
                Entry::Chunk { .. } => continue,
            };
            match entry.parent() {
                Some(parent) if parent.get() >= floor => {
                    let at = structural[..offset]
                        .binary_search(&parent)
                        .expect("BUG: an entry's parent effect precedes it in its scope");
                    inside[at] = inside[at].join(scope);
                }
                _ => whole = whole.join(scope),
            }
        }
        whole
    }

    /// What the child boundary `child` slotted here draws, in its parent's
    /// space, from its stored bounds.
    fn slot_scope(self, child: NodeKey) -> Scope {
        let child = self.record(child);
        Scope {
            local: map_out(
                &[Link::Matrix(placement(child))],
                Reach::of(child.subtree()),
            ),
            kernel: child.kernel(),
        }
    }

    /// Whether the child boundary `child` draws the same in its parent's
    /// space under both versions: its placement, its subtree bounds and its
    /// kernel, from which its slot's scope is read.
    pub(crate) fn same_slot(self, other: Geometry<'_>, child: NodeKey) -> bool {
        match (self.version.record(child), other.version.record(child)) {
            (Some(one), Some(two)) => {
                one.placement() == two.placement()
                    && one.subtree() == two.subtree()
                    && one.kernel().to_bits() == two.kernel().to_bits()
            }
            _ => false,
        }
    }

    /// What `key`'s content and subtree change, in its own space, from its
    /// children's stored bounds, and its filters' device kernel.
    pub(crate) fn subtree_scope(self, key: NodeKey) -> Scope {
        self.scope(key, None, None)
    }

    /// Where `entry` of `key` and its scope land on the surface; nothing when
    /// the node is not attached.
    pub(crate) fn entry_reach(self, key: NodeKey, entry: EntryIndex) -> Reach {
        let mut links = Links::new();
        if self.node_links(key, &mut links, 0).is_err() {
            return Reach::Nothing;
        }
        self.content_links(key, entry, &mut links, 0);
        let scope = self.scope(key, Some(entry), None);
        map_out_spread(&links, scope.local, scope.kernel)
    }

    /// The links from the surface into the space that holds `entry` of
    /// `key`: the node's chain, then the effects above the entry; `None`
    /// when the node is not attached.
    pub(crate) fn entry_chain(self, key: NodeKey, entry: EntryIndex) -> Option<Links<'v>> {
        let mut links = Links::new();
        self.node_links(key, &mut links, 0).ok()?;
        self.content_links(key, entry, &mut links, 0);
        Some(links)
    }

    /// Where the effect `entry` of `key` and its scope land on the surface,
    /// given what it holds under its own map ([`Self::effect_inside`]) and
    /// the chain into the space that holds it ([`Self::entry_chain`]).
    pub(crate) fn effect_reach(
        self,
        chain: &[Link<'_>],
        key: NodeKey,
        entry: EntryIndex,
        inside: Scope,
    ) -> Reach {
        let mut own = Links::new();
        if let Some(layer) = self.record(key).content().effect_layer(entry) {
            self.effect_links(layer, Address { node: key, entry }, &mut own, 0);
        }
        let scope = inside.through(&own);
        map_out_spread(chain, scope.local, scope.kernel)
    }

    /// The node whose content slots `key`.
    pub(crate) fn parent(self, key: NodeKey) -> Option<NodeKey> {
        self.version.record(key).and_then(NodeRecord::parent)
    }

    /// Whether `key`'s content holds a follower, whose transform each
    /// version resolves on its own.
    pub(crate) fn holds_followers(self, key: NodeKey) -> bool {
        self.version.record(key).is_some_and(|record| {
            record
                .content()
                .links()
                .iter()
                .any(|(_, role, _)| *role == LinkRole::Follower)
        })
    }

    /// Where `key`'s subtree lands on the surface, from its stored bounds;
    /// nothing when it is not attached.
    pub(crate) fn subtree_reach(self, key: NodeKey) -> Reach {
        let mut links = Links::new();
        if self.node_links(key, &mut links, 0).is_err() {
            return Reach::Nothing;
        }
        let record = self.record(key);
        map_out_spread(&links, Reach::of(record.subtree()), record.kernel())
    }

    /// The map from the space under the effect at `at` (its own map applied)
    /// to the surface; `None` when nothing there is drawn.
    pub(crate) fn effect_ctm(self, at: Address) -> Option<Matrix4> {
        let mut links = Links::new();
        self.node_links(at.node, &mut links, 0).ok()?;
        self.content_links(at.node, at.entry, &mut links, 0);
        if let Some(layer) = self.record(at.node).content().effect_layer(at.entry) {
            self.effect_links(layer, at, &mut links, 0);
        }
        product(&links).ok()
    }

    /// What each foreground image filter of `key` takes in, on the surface
    /// and unclipped, with the map under it, from one fold of the content;
    /// `reads` counts the entries the fold covers. A filter whose space
    /// nothing draws in is left out.
    pub(crate) fn filter_inputs(
        self,
        key: NodeKey,
        reads: &mut u64,
    ) -> SmallVec<[(Address, Matrix4, Reach); 4]> {
        let entries = self.record(key).content().entries().len();
        let mut filters = Vec::new();
        self.scope(key, None, Some(&mut filters));
        *reads = reads.saturating_add(u64::try_from(entries).unwrap_or(u64::MAX));
        filters
            .into_iter()
            .filter_map(|(entry, inside)| {
                let at = Address { node: key, entry };
                let ctm = self.effect_ctm(at)?;
                let input = match inside.local {
                    Reach::Rect(rect) => mapped(&ctm, rect).spread(inside.kernel),
                    other => other,
                };
                Some((at, ctm, input))
            })
            .collect()
    }

    /// The transform the follower at `at` applies to its children so their
    /// content lands on its leader: `F⁻¹ · L · translate(linked_offset)`,
    /// with `F` the map of the follower's parent space and `L` the leader's,
    /// its own offset applied, both from their lowest common ancestor. This is
    /// `resolve_follower_transform`'s rule (ADR-0276), its nesting counted
    /// the same way. A leader clip that hides the follower is not consulted: a
    /// footprint that shows a hidden follower over-reports, and a watching
    /// reader reports each one under `LeaderClip::Hide` whose leader's branch
    /// the patch changed, this one or one on either chain.
    pub(crate) fn resolve(self, at: Address, nesting: usize) -> Result<Matrix4, Unresolved> {
        if let Some(watch) = self.watch {
            watch
                .resolutions
                .set(watch.resolutions.get().saturating_add(1));
        }
        if nesting > MAX_FOLLOWER_NESTING {
            return Err(Unresolved::TooDeep);
        }
        // A follower on either chain resolves one level deeper, as
        // `resolve_follower_transform` counts it.
        let chains = nesting + 1;
        let Some(Layer::Follower(follower)) = self.record(at.node).content().effect_layer(at.entry)
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
        let Some(leader_at) = self.leader(follower.link()) else {
            return unlinked();
        };
        let Some(Layer::Leader(leader)) = self
            .record(leader_at.node)
            .content()
            .effect_layer(leader_at.entry)
        else {
            return unlinked();
        };
        if let Some(watch) = self.watch
            && follower.leader_clip() == LeaderClip::Hide
            && self.path_up(leader_at.node).any(watch.changed)
        {
            watch.hiding_changed.set(true);
        }
        if self.is_inside(leader_at, at) {
            tracing::warn!(
                "a leader is inside its follower's subtree (a leader must precede its \
                 follower in paint order); using the unlinked contract",
            );
            return unlinked();
        }
        // Both chains stop at the lowest common ancestor, as
        // `resolve_follower_transform` walks them: the shared part would
        // cancel, and resolving the followers on it again for each chain
        // multiplies the work by every level of nesting.
        let Ok(mut leader_up) = self.ancestry(leader_at) else {
            return unlinked();
        };
        leader_up.insert(0, Up::Effect(leader_at));
        let mut follower_up = self.ancestry(at).map_err(|Unattached| Unresolved::Hidden)?;
        while leader_up.last().is_some() && leader_up.last() == follower_up.last() {
            leader_up.pop();
            follower_up.pop();
        }
        if self.clip_hides && follower.leader_clip() == LeaderClip::Hide {
            self.admit_anchor(
                &leader_up,
                follower.leader_anchor().along_size(leader.size()),
                chains,
            )?;
        }
        let mut leader_links = Links::new();
        self.up_links(&leader_up, &mut leader_links, chains);
        let mut follower_links = Links::new();
        self.up_links(&follower_up, &mut follower_links, chains);
        let leader_ctm = product(&leader_links)?;
        let follower_ctm = product(&follower_links)?;
        let linked = follower.linked_offset(leader.size());
        let transform = invert(&follower_ctm).ok_or(Unresolved::Hidden)?
            * leader_ctm
            * Matrix4::translation(linked.dx, linked.dy, 0.0);
        invert(&transform).ok_or(Unresolved::Hidden)?;
        finite(transform)
    }

    /// The steps above the entry at `at` up to the surface, innermost first:
    /// each effect over it in its node, the node's placement, then the
    /// effects over the slot that names it, and so on to the root's
    /// placement.
    fn ancestry(self, at: Address) -> Result<SmallVec<[Up; 16]>, Unattached> {
        let mut steps = SmallVec::new();
        let (mut node, mut entry) = (at.node, at.entry);
        loop {
            let content = self.record(node).content();
            let mut above = content.entries()[entry.get()].parent();
            while let Some(effect) = above {
                steps.push(Up::Effect(Address {
                    node,
                    entry: effect,
                }));
                above = content.entries()[effect.get()].parent();
            }
            steps.push(Up::Place(node));
            match self.version.record(node).and_then(NodeRecord::parent) {
                Some(parent) => {
                    entry = self
                        .record(parent)
                        .content()
                        .slot_entry(node)
                        .expect("BUG: a node's parent names it in a slot");
                    node = parent;
                }
                None if node == self.version.root => return Ok(steps),
                None => return Err(Unattached),
            }
        }
    }

    /// Where the leader publishing `link` has its top-left corner on the
    /// surface: `None` without an attached leader, or when its chain is
    /// singular, not finite or holds a follower that resolves to nothing.
    pub(crate) fn leader_origin(self, link: LayerLink) -> Option<Point<f64>> {
        let at = self.leader(link)?;
        let mut steps = self.ancestry(at).ok()?;
        steps.insert(0, Up::Effect(at));
        let mut links = Links::new();
        // A follower on the chain resolves one level deep, as `leader_origin`
        // over a layer tree counts it.
        self.up_links(&steps, &mut links, 1);
        let (x, y) = product(&links).ok()?.project_plane_point(0.0, 0.0)?;
        (x.is_finite() && y.is_finite()).then(|| Point::new(x, y))
    }

    /// Whether every clip above the leader on its chain (`steps`, innermost
    /// first and the leader itself first of all) admits the leader's anchor
    /// point, projected through the chain below that clip, as
    /// `resolve_follower_transform` tests it: `Hidden` when one leaves it out.
    fn admit_anchor(self, steps: &[Up], anchor: Offset, nesting: usize) -> Result<(), Unresolved> {
        let mut chain = Matrix4::IDENTITY;
        for (index, step) in steps.iter().enumerate() {
            let mut links = Links::new();
            match *step {
                Up::Place(node) => links.push(Link::Matrix(placement(self.record(node)))),
                Up::Effect(at) => {
                    let Some(layer) = self.record(at.node).content().effect_layer(at.entry) else {
                        continue;
                    };
                    if index > 0 {
                        let (x, y) = chain
                            .project_plane_point(anchor.dx, anchor.dy)
                            .ok_or(Unresolved::Hidden)?;
                        if !clip_admits(layer, Point::new(x, y)) {
                            return Err(Unresolved::Hidden);
                        }
                    }
                    self.effect_links(layer, at, &mut links, nesting);
                }
            }
            chain = product(&links)? * chain;
        }
        Ok(())
    }

    /// The links of `steps` (innermost first), outermost first.
    fn up_links(self, steps: &[Up], out: &mut Links<'v>, nesting: usize) {
        for step in steps.iter().rev() {
            match *step {
                Up::Place(node) => out.push(Link::Matrix(placement(self.record(node)))),
                Up::Effect(at) => {
                    if let Some(layer) = self.record(at.node).content().effect_layer(at.entry) {
                        self.effect_links(layer, at, out, nesting);
                    }
                }
            }
        }
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

/// The link a recording's damage extent gives: nothing when it draws
/// nothing, everywhere when its extent has no finite cover.
fn extent_link<'v>(extent: Option<DamageExtent>) -> Link<'v> {
    match extent.map(DamageExtent::covering_rect) {
        None => Link::Extent(Rect::ZERO),
        Some(Some(rect)) if rect.is_finite() => Link::Extent(rect),
        Some(_) => Link::Everywhere,
    }
}
