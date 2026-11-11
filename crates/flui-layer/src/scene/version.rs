//! A persistent scene version and its indexes (ADR-0304 §1).

use std::sync::Arc;

use flui_painting::paint::TextureId;
use smallvec::SmallVec;

use flui_foundation::geometry::{Matrix4, Point, Rect};

use super::content::{BoundaryContent, Bounds, Entry, EntryIndex, LinkRole};
use super::geometry::Geometry;
use super::id::{Lineage, NodeKey, SceneNodeId, SceneVersionId};
use super::owner::Placement;
use super::table::{Table, TableCost};
use crate::link::LayerLink;

/// Where an entry sits: its boundary's node and its index in that content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryAddress {
    /// The boundary.
    pub node: SceneNodeId,
    /// The entry in the boundary's content.
    pub entry: EntryIndex,
}

/// An entry's address inside its version, whose lineage it shares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Address {
    pub(crate) node: NodeKey,
    pub(crate) entry: EntryIndex,
}

/// One boundary as the table stores it: every reference is a key of the
/// version's own lineage.
#[derive(Debug, Clone)]
pub(crate) struct NodeRecord {
    key: NodeKey,
    parent: Option<NodeKey>,
    depth: u32,
    placement: Placement,
    content: Arc<BoundaryContent>,
    /// What the node and its subtree can change, in its own space: exact
    /// when its content is published, grown by union after (ADR-0304 §4).
    subtree: Bounds,
    /// How far, in device pixels, the filters in the subtree spread past
    /// `subtree` once it is mapped to the surface; grown with it.
    kernel: f64,
}

impl NodeRecord {
    pub(crate) fn new(key: NodeKey, content: Arc<BoundaryContent>) -> Self {
        Self {
            key,
            parent: None,
            depth: 0,
            placement: Placement::ZERO,
            content,
            subtree: Bounds::Finite(Rect::ZERO),
            kernel: 0.0,
        }
    }

    pub(crate) fn parent(&self) -> Option<NodeKey> {
        self.parent
    }

    pub(crate) fn depth(&self) -> u32 {
        self.depth
    }

    pub(crate) fn content(&self) -> &Arc<BoundaryContent> {
        &self.content
    }

    pub(crate) fn set_parent(&mut self, parent: Option<NodeKey>) {
        self.parent = parent;
    }

    pub(crate) fn set_depth(&mut self, depth: u32) {
        self.depth = depth;
    }

    pub(crate) fn set_placement(&mut self, placement: Placement) {
        self.placement = placement;
    }

    pub(crate) fn placement(&self) -> Placement {
        self.placement
    }

    pub(crate) fn subtree(&self) -> Bounds {
        self.subtree
    }

    pub(crate) fn set_subtree(&mut self, subtree: Bounds) {
        self.subtree = subtree;
    }

    pub(crate) fn kernel(&self) -> f64 {
        self.kernel
    }

    pub(crate) fn set_kernel(&mut self, kernel: f64) {
        self.kernel = kernel;
    }

    pub(crate) fn set_content(&mut self, content: Arc<BoundaryContent>) {
        self.content = content;
    }
}

/// One boundary in a version, read through the version that holds it.
#[derive(Debug, Clone, Copy)]
pub struct SceneNode<'a> {
    lineage: Lineage,
    record: &'a NodeRecord,
}

impl<'a> SceneNode<'a> {
    /// The node's id.
    #[must_use]
    pub fn id(self) -> SceneNodeId {
        SceneNodeId::new(self.lineage, self.record.key)
    }

    /// The node whose content names it in a slot, `None` for the root and an
    /// unattached node.
    #[must_use]
    pub fn parent(self) -> Option<SceneNodeId> {
        self.record
            .parent
            .map(|parent| SceneNodeId::new(self.lineage, parent))
    }

    /// How many parents lie above it: 0 for the root and for the top of an
    /// unattached subtree.
    #[must_use]
    pub fn depth(self) -> u32 {
        self.record.depth
    }

    /// Its offset in its parent's slot space.
    #[must_use]
    pub fn placement(self) -> Placement {
        self.record.placement
    }

    /// What it paints.
    #[must_use]
    pub fn content(self) -> &'a Arc<BoundaryContent> {
        &self.record.content
    }
}

/// The users of one link, each in the order it entered the index.
#[derive(Debug, Clone, Default)]
pub(crate) struct LinkUsers {
    pub(crate) leaders: SmallVec<[Address; 1]>,
    pub(crate) followers: SmallVec<[Address; 2]>,
}

impl LinkUsers {
    fn is_empty(&self) -> bool {
        self.leaders.is_empty() && self.followers.is_empty()
    }
}

/// The entries that draw one texture.
pub(crate) type TextureUsers = SmallVec<[Address; 2]>;

/// One immutable state of a retained scene.
///
/// Cloning is one reference-count increment; a version keeps every node it
/// was applied with whatever its owner does next, and shares each node it
/// did not change with the version before it.
#[derive(Debug, Clone)]
pub struct SceneVersion(Arc<VersionInner>);

#[derive(Debug, Clone)]
pub(crate) struct VersionInner {
    pub(crate) id: SceneVersionId,
    pub(crate) root: NodeKey,
    /// By slot.
    pub(crate) nodes: Table<NodeRecord>,
    /// By `LayerLink` id.
    pub(crate) links: Table<LinkUsers>,
    /// By `TextureId` value.
    pub(crate) textures: Table<TextureUsers>,
    /// The nodes whose content holds an effect that reads pixels outside the
    /// content under it, by slot.
    pub(crate) filtered: Table<NodeKey>,
}

impl SceneVersion {
    pub(crate) fn from_inner(inner: VersionInner) -> Self {
        Self(Arc::new(inner))
    }

    pub(crate) fn inner(&self) -> &VersionInner {
        &self.0
    }

    fn lineage(&self) -> Lineage {
        self.0.id.lineage()
    }

    fn address(&self, address: Address) -> EntryAddress {
        EntryAddress {
            node: SceneNodeId::new(self.lineage(), address.node),
            entry: address.entry,
        }
    }

    /// Which version this is.
    #[must_use]
    pub fn id(&self) -> SceneVersionId {
        self.0.id
    }

    /// The node every attached boundary hangs under.
    #[must_use]
    pub fn root(&self) -> SceneNodeId {
        SceneNodeId::new(self.lineage(), self.0.root)
    }

    /// How many nodes the version holds, unattached ones included.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.nodes.len()
    }

    /// Whether the version holds no node; never true, since it holds its
    /// root.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.nodes.len() == 0
    }

    /// The node `id` names, `None` when the version holds no node under that
    /// id: another lineage's id and a stale generation included.
    #[must_use]
    pub fn node(&self, id: SceneNodeId) -> Option<SceneNode<'_>> {
        if id.lineage() != self.lineage() {
            return None;
        }
        self.0.record(id.key()).map(|record| SceneNode {
            lineage: self.lineage(),
            record,
        })
    }

    /// Every node, by slot.
    pub fn nodes(&self) -> impl Iterator<Item = SceneNode<'_>> + '_ {
        let lineage = self.lineage();
        self.0
            .nodes
            .iter()
            .map(move |(_, record)| SceneNode { lineage, record })
    }

    /// The leader entries publishing `link`, in the order they entered the
    /// index, unattached nodes included; a scene resolves a follower against
    /// the last attached one.
    pub fn leaders(&self, link: LayerLink) -> impl Iterator<Item = EntryAddress> + '_ {
        self.0
            .links
            .get(link.id())
            .into_iter()
            .flat_map(|users| users.leaders.iter().map(|address| self.address(*address)))
    }

    /// The follower entries positioned against `link`.
    pub fn followers(&self, link: LayerLink) -> impl Iterator<Item = EntryAddress> + '_ {
        self.0
            .links
            .get(link.id())
            .into_iter()
            .flat_map(|users| users.followers.iter().map(|address| self.address(*address)))
    }

    /// The entries that draw `texture`.
    pub fn texture_users(&self, texture: TextureId) -> impl Iterator<Item = EntryAddress> + '_ {
        self.0
            .textures
            .get(texture.get())
            .into_iter()
            .flat_map(|users| users.iter().map(|address| self.address(*address)))
    }

    /// The transform the renderer applies at the follower entry `at` so its
    /// content lands on its leader: what [`resolve_follower_transform`]
    /// answers over [`Self::flatten`]'s tree, read off the version without
    /// building one. `None` when nothing is drawn or hit there: `at` is not an
    /// attached follower of this version, it is unlinked and hidden, a clip
    /// on the leader's chain leaves out its anchor under
    /// [`LeaderClip::Hide`](crate::LeaderClip::Hide), a chain is singular or
    /// not finite, or followers nest past the limit.
    ///
    /// [`resolve_follower_transform`]: crate::resolve_follower_transform
    #[must_use]
    pub fn follower_transform(&self, at: EntryAddress) -> Option<Matrix4> {
        if at.node.lineage() != self.lineage() {
            return None;
        }
        let record = self.0.record(at.node.key())?;
        if !matches!(
            record.content().entries().get(at.entry.get()),
            Some(Entry::Effect {
                layer: crate::Layer::Follower(_),
                ..
            })
        ) {
            return None;
        }
        let geometry = Geometry::shown(&self.0);
        if !geometry.is_attached(at.node.key()) {
            return None;
        }
        geometry
            .resolve(
                Address {
                    node: at.node.key(),
                    entry: at.entry,
                },
                0,
            )
            .ok()
    }

    /// Where the leader publishing `link` has its top-left corner on the
    /// surface, as [`leader_origin`](crate::leader_origin) answers over
    /// [`Self::flatten`]'s tree: `None` without an attached leader, or when a
    /// transform on its chain is singular or not finite, or a follower on it
    /// resolves to nothing.
    #[must_use]
    pub fn leader_origin(&self, link: LayerLink) -> Option<Point<f64>> {
        Geometry::shown(&self.0).leader_origin(link)
    }

    /// The nodes whose content holds an effect that reads pixels outside the
    /// content under it (a backdrop or image filter), by slot.
    pub fn filtered_nodes(&self) -> impl Iterator<Item = SceneNodeId> + '_ {
        let lineage = self.lineage();
        self.0
            .filtered
            .iter()
            .map(move |(_, key)| SceneNodeId::new(lineage, *key))
    }
}

impl VersionInner {
    /// The record `key` names, `None` for an empty slot or another
    /// generation.
    pub(crate) fn record(&self, key: NodeKey) -> Option<&NodeRecord> {
        self.nodes
            .get(key.table_key())
            .filter(|record| record.key == key)
    }

    /// Takes every address of a removed node out of the named link and
    /// texture buckets, filtering each bucket once however many removed
    /// entries it held. `links` and `textures` are sorted and deduplicated.
    pub(crate) fn unindex(
        &mut self,
        links: &[u64],
        textures: &[u64],
        removed: impl Fn(NodeKey) -> bool,
        cost: &mut TableCost,
    ) {
        let kept = |address: &Address| !removed(address.node);
        for &link in links {
            if let Some(users) = self.links.get_mut(link, cost) {
                users.leaders.retain(|address| kept(address));
                users.followers.retain(|address| kept(address));
                if users.is_empty() {
                    self.links.remove(link, cost);
                }
            }
        }
        for &texture in textures {
            if let Some(users) = self.textures.get_mut(texture, cost) {
                users.retain(|address| kept(address));
                if users.is_empty() {
                    self.textures.remove(texture, cost);
                }
            }
        }
    }

    /// Puts `node`'s entries in `content` into the three indexes.
    pub(crate) fn index(&mut self, node: NodeKey, content: &BoundaryContent, cost: &mut TableCost) {
        // Each bucket is unshared once and extended in place, so entries that
        // share a link or a texture cost one copy, not one per entry.
        for (link, role, entry) in content.links() {
            let address = Address {
                node,
                entry: *entry,
            };
            let users = self.links.get_or_default(link.id(), cost);
            match role {
                LinkRole::Leader => users.leaders.push(address),
                LinkRole::Follower => users.followers.push(address),
            }
        }
        for (texture, entry) in content.textures() {
            self.textures
                .get_or_default(texture.get(), cost)
                .push(Address {
                    node,
                    entry: *entry,
                });
        }
        if content.is_filtered() {
            self.filtered.insert(node.table_key(), node, cost);
        }
    }
}

#[cfg(feature = "testing")]
mod flatten {
    use flui_foundation::LayerId;

    use super::SceneVersion;
    use crate::layer::{Layer, OffsetLayer, PictureLayer};
    use crate::scene::content::Entry;
    use crate::tree::LayerTree;

    impl SceneVersion {
        /// The layer tree the version paints from its root: each node an
        /// [`OffsetLayer`] at its placement, each effect its layer, each chunk
        /// a [`PictureLayer`]. Unattached nodes are left out.
        ///
        /// The oracle a retained frame is compared against
        /// (ADR-0304 §11).
        #[must_use]
        pub fn flatten(&self) -> LayerTree {
            let root = self
                .node(self.root())
                .expect("BUG: a version holds its root");
            let mut tree = LayerTree::new(Layer::from(OffsetLayer::new(root.placement().offset())));
            let mut pending = vec![(self.root(), tree.root())];
            while let Some((id, at)) = pending.pop() {
                let node = self
                    .node(id)
                    .expect("BUG: a slot names a node of its version");
                let mut placed: Vec<LayerId> = Vec::with_capacity(node.content().entries().len());
                for entry in node.content().entries() {
                    let parent = entry.parent().map_or(at, |index| placed[index.get()]);
                    let layer = match entry {
                        Entry::Effect { layer, .. } => tree.push_child(parent, layer.clone()),
                        Entry::Chunk { chunk, .. } => tree.push_child(
                            parent,
                            Layer::from(PictureLayer::new(chunk.commands().clone())),
                        ),
                        Entry::Slot { node: child, .. } => {
                            let placement = self
                                .node(*child)
                                .expect("BUG: a slot names a node of its version")
                                .placement();
                            let offset = tree.push_child(
                                parent,
                                Layer::from(OffsetLayer::new(placement.offset())),
                            );
                            pending.push((*child, offset));
                            offset
                        }
                    };
                    placed.push(layer);
                }
            }
            tree
        }
    }
}
