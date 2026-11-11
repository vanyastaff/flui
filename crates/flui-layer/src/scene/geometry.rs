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
use std::ops::Range;

use flui_foundation::geometry::{Matrix4, Offset, Point, Rect};
use flui_painting::DamageExtent;
use flui_painting::paint::ImageFilter;
use smallvec::SmallVec;

use super::content::{BoundaryContent, Bounds, Entry, EntryIndex};
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
        Reach::Rect(rect) => mapped(&ctm, rect).spread(kernel),
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
                Reach::Rect(input) => match spread {
                    Spread::Reach => footprint(filter, input, ctm),
                    Spread::Kernel => moved(filter, input, ctm),
                }
                .map_or(Reach::Unbounded, Reach::rect),
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

    /// What the entries of `key` in `range`, one effect's whole scope or the
    /// whole content, draw in the space that holds them (`key`'s own space
    /// for a top-level entry), and how far, in device pixels, the filters
    /// among them spread it once mapped to the surface (`device_spread`),
    /// slotted subtrees included. One pass from the last entry back folds
    /// each into its parent's, so a chain of effects of any depth costs no
    /// stack.
    fn scope(
        self,
        key: NodeKey,
        range: Range<usize>,
        mut filters: Option<&mut Vec<(EntryIndex, Scope)>>,
    ) -> Scope {
        let content = self.record(key).content();
        let entries = &content.entries()[range.clone()];
        let mut inside = vec![Scope::EMPTY; entries.len()];
        let mut whole = Scope::EMPTY;
        for (offset, entry) in entries.iter().enumerate().rev() {
            let scope = match entry {
                Entry::Chunk { chunk, .. } => Scope {
                    local: Reach::of(chunk.bounds()),
                    kernel: 0.0,
                },
                Entry::Slot { node, .. } => {
                    let child = self.record(node.key());
                    Scope {
                        local: map_out(
                            &[Link::Matrix(placement(child))],
                            Reach::of(child.subtree()),
                        ),
                        kernel: child.kernel(),
                    }
                }
                Entry::Effect { layer, .. } => {
                    let held = inside[offset];
                    let mut links = Links::new();
                    let at = Address {
                        node: key,
                        entry: EntryIndex::at(range.start + offset),
                    };
                    self.effect_links(layer, at, &mut links, 0);
                    if let Layer::ImageFilter(_) = layer
                        && let Some(filters) = filters.as_deref_mut()
                    {
                        filters.push((at.entry, held));
                    }
                    held.through(&links)
                }
            };
            match entry.parent() {
                Some(parent) if parent.get() >= range.start => {
                    let held = &mut inside[parent.get() - range.start];
                    *held = held.join(scope);
                }
                _ => whole = whole.join(scope),
            }
        }
        whole
    }

    /// What `key`'s content and subtree change, in its own space, from its
    /// children's stored bounds, and its filters' device kernel.
    pub(crate) fn subtree_scope(self, key: NodeKey) -> Scope {
        self.scope(key, 0..self.record(key).content().entries().len(), None)
    }

    /// Where `entry` of `key` and its scope land on the surface; nothing when
    /// the node is not attached.
    pub(crate) fn entry_reach(self, key: NodeKey, entry: EntryIndex) -> Reach {
        let mut links = Links::new();
        if self.node_links(key, &mut links, 0).is_err() {
            return Reach::Nothing;
        }
        self.content_links(key, entry, &mut links, 0);
        let scope = self.scope(
            key,
            entry.get()..scope_end(self.record(key).content(), entry),
            None,
        );
        map_out_spread(&links, scope.local, scope.kernel)
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
        if let Entry::Effect { layer, .. } =
            &self.record(at.node).content().entries()[at.entry.get()]
        {
            self.effect_links(layer, at, &mut links, 0);
        }
        product(&links).ok()
    }

    /// What each foreground image filter of `key` takes in, on the surface
    /// and unclipped, with the map under it, from one fold of the content;
    /// `reads` counts the entries folded. A filter whose space nothing
    /// draws in is left out.
    pub(crate) fn filter_inputs(
        self,
        key: NodeKey,
        reads: &mut u64,
    ) -> SmallVec<[(Address, Matrix4, Reach); 4]> {
        let entries = self.record(key).content().entries().len();
        let mut filters = Vec::new();
        self.scope(key, 0..entries, Some(&mut filters));
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
        let Some(leader_at) = self.leader(follower.link()) else {
            return unlinked();
        };
        let Entry::Effect {
            layer: Layer::Leader(leader),
            ..
        } = &self.record(leader_at.node).content().entries()[leader_at.entry.get()]
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
                    let Entry::Effect { layer, .. } =
                        &self.record(at.node).content().entries()[at.entry.get()]
                    else {
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
                    if let Entry::Effect { layer, .. } =
                        &self.record(at.node).content().entries()[at.entry.get()]
                    {
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

/// The position after the scope of `entry`: an effect's scope follows it,
/// and the first entry whose parent precedes `entry` or that has none ends
/// it.
fn scope_end(content: &BoundaryContent, entry: EntryIndex) -> usize {
    let entries = content.entries();
    (entry.get() + 1..entries.len())
        .find(|&position| {
            entries[position]
                .parent()
                .is_none_or(|parent| parent.get() < entry.get())
        })
        .unwrap_or(entries.len())
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
