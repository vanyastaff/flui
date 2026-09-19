//! The per-frame compositor tree: an append-only arena of [`LayerNode`]s.
//!
//! A `LayerTree` is built once per frame, pre-order, by the paint walk
//! (`flui-rendering`'s fragment composer), frozen into a [`Scene`], and walked
//! by the GPU backend. It is born with its root and nothing is ever removed or
//! re-parented: the only way to add a node is [`LayerTree::push_child`], which
//! mints the child's id in the same call that links it. A fresh id has no descendants, so a cycle or a
//! double-parented node cannot be expressed through the public API, and no
//! walker needs a visited set. A node is written exactly once, at insertion:
//! there is no `&mut` reach into a node, so the leader index the tree builds
//! then can never go stale.
//!
//! [`Scene`]: crate::Scene

use std::collections::HashMap;

use flui_foundation::{Diagnosticable, LayerId, RenderId};

use crate::{LayerLink, layer::Layer};

/// A [`Layer`] plus its position in the tree.
#[derive(Debug)]
pub struct LayerNode {
    parent: Option<LayerId>,
    children: Vec<LayerId>,
    layer: Layer,
    /// The repaint boundary whose paint produced this layer, when one did.
    ///
    /// This is what lets two consecutive frames be compared: each frame builds
    /// a fresh tree with fresh ids, so `LayerId` pairs nothing, and damage has
    /// to come from comparing layer trees rather than from which render
    /// objects repainted (ADR-0061). `None` on every layer no boundary
    /// originated — structural layers a fragment pushed — so a pairing pass
    /// cannot mistake one for a boundary. The root carries a stamp only when
    /// the root render object is itself a repaint boundary (as `RenderView`
    /// is); a plain `RenderFlex` root, as most test fixtures mount, leaves it
    /// `None`.
    render_id: Option<RenderId>,
}

impl Diagnosticable for LayerNode {
    /// Names the node by its layer kind and merges the layer's own properties
    /// with the boundary stamp.
    fn to_diagnostics_node(&self) -> flui_foundation::DiagnosticsNode {
        let mut node = self.layer.to_diagnostics_node();
        if let Some(render_id) = self.render_id {
            node = node.property("render_id", format!("{render_id:?}"));
        }
        node
    }
}

impl LayerNode {
    /// Wraps a layer with no parent, no children, and no boundary stamp.
    pub fn new(layer: Layer) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            layer,
            render_id: None,
        }
    }

    /// Stamps this node with the repaint boundary that produced it — see
    /// [`Self::render_id`].
    #[must_use]
    pub fn with_render_id(mut self, render_id: RenderId) -> Self {
        self.render_id = Some(render_id);
        self
    }

    /// The parent's id; `None` only for the root.
    #[inline]
    pub fn parent(&self) -> Option<LayerId> {
        self.parent
    }

    /// The children in paint order.
    #[inline]
    pub fn children(&self) -> &[LayerId] {
        &self.children
    }

    /// The layer this node carries.
    #[inline]
    pub fn layer(&self) -> &Layer {
        &self.layer
    }

    /// The repaint boundary this layer came from — see [`Self::render_id`].
    #[inline]
    pub fn render_id(&self) -> Option<RenderId> {
        self.render_id
    }
}

impl From<Layer> for LayerNode {
    fn from(layer: Layer) -> Self {
        Self::new(layer)
    }
}

/// The compositor tree: the fourth of FLUI's five trees.
///
/// Nodes live in a `Vec` indexed by `LayerId - 1` (public ids are 1-based
/// `NonZeroUsize`, storage is 0-based — the workspace's ID offset pattern).
/// Every id ever returned stays valid for the tree's lifetime.
///
/// `LayerTree` is `Send + Sync` by construction but has a single mutable
/// owner: it is built on one thread, frozen into a [`Scene`](crate::Scene),
/// and read on another. Multi-output rendering means multiple trees, not a
/// shared one.
///
/// ```rust
/// use flui_layer::{Layer, LayerTree, OffsetLayer, PictureLayer};
///
/// let mut tree = LayerTree::new(Layer::from(OffsetLayer::zero()));
/// let root = tree.root();
/// let leaf = tree.push_child(root, Layer::from(PictureLayer::default()));
///
/// assert_eq!(tree.parent(leaf), Some(root));
/// assert_eq!(tree.children(root), Some(&[leaf][..]));
/// ```
#[derive(Debug)]
pub struct LayerTree {
    /// Never empty: slot 0 is the root, placed by [`LayerTree::new`].
    nodes: Vec<LayerNode>,
    /// `LayerLink → LeaderLayer` for this frame, filled as leaders are pushed.
    ///
    /// The link is a `Copy` token, so the tree holds the index. Two leaders on
    /// one link in one frame is a widget-tree error; a debug build trips on
    /// it, a release build keeps the later leader.
    leaders: HashMap<LayerLink, LayerId>,
}

#[inline]
fn slot(id: LayerId) -> usize {
    id.get() - 1
}

#[inline]
fn id_at(slot: usize) -> LayerId {
    LayerId::new(slot + 1)
}

/// The id every tree's root has: the first slot minted.
const ROOT: LayerId = match LayerId::new_checked(1) {
    Some(id) => id,
    None => unreachable!(),
};

impl Default for LayerTree {
    /// A tree whose root is a zero [`OffsetLayer`](crate::OffsetLayer) with no
    /// children — a frame that paints nothing.
    fn default() -> Self {
        Self::new(Layer::Offset(crate::OffsetLayer::zero()))
    }
}

impl LayerTree {
    /// A tree of one node, `root`.
    ///
    /// Every other node enters through [`Self::push_child`], so a tree has
    /// exactly one root for its whole life; there is no empty state to check
    /// for and [`Self::root`] is infallible.
    pub fn new(root: impl Into<LayerNode>) -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            leaders: HashMap::new(),
        };
        let id = tree.push(root.into(), None);
        debug_assert_eq!(id, ROOT);
        tree
    }

    /// The root's id.
    #[inline]
    pub fn root(&self) -> LayerId {
        ROOT
    }

    /// Appends `node` as the last child of `parent` and returns its id.
    ///
    /// Both links are set here and nowhere else, which is what makes the
    /// structural invariants hold by construction: the child's id is fresh,
    /// so it can be neither an ancestor of `parent` nor already parented.
    ///
    /// # Panics
    ///
    /// If `parent` is not an id this tree returned.
    pub fn push_child(&mut self, parent: LayerId, node: impl Into<LayerNode>) -> LayerId {
        assert!(
            self.contains(parent),
            "BUG: LayerTree::push_child: parent is not a node of this tree"
        );
        let id = self.push(node.into(), Some(parent));
        self.nodes[slot(parent)].children.push(id);
        id
    }

    fn push(&mut self, mut node: LayerNode, parent: Option<LayerId>) -> LayerId {
        let id = id_at(self.nodes.len());
        node.parent = parent;
        if let Layer::Leader(leader) = &node.layer {
            debug_assert!(
                !self.leaders.contains_key(&leader.link()),
                "BUG: two LeaderLayers share {:?} in one frame",
                leader.link()
            );
            self.leaders.insert(leader.link(), id);
        }
        self.nodes.push(node);
        id
    }

    /// The node behind `id`.
    #[inline]
    pub fn get(&self, id: LayerId) -> Option<&LayerNode> {
        self.nodes.get(slot(id))
    }

    /// The layer behind `id`.
    #[inline]
    pub fn get_layer(&self, id: LayerId) -> Option<&Layer> {
        self.get(id).map(LayerNode::layer)
    }

    /// The parent of `id`; `None` for the root or an unknown id.
    #[inline]
    pub fn parent(&self, id: LayerId) -> Option<LayerId> {
        self.get(id).and_then(LayerNode::parent)
    }

    /// The children of `id` in paint order; `None` for an unknown id.
    #[inline]
    pub fn children(&self, id: LayerId) -> Option<&[LayerId]> {
        self.get(id).map(LayerNode::children)
    }

    /// The `LeaderLayer` registered for `link` in this frame, if any.
    #[inline]
    pub fn leader(&self, link: LayerLink) -> Option<LayerId> {
        self.leaders.get(&link).copied()
    }

    /// Whether `id` names a node of this tree.
    #[inline]
    pub fn contains(&self, id: LayerId) -> bool {
        slot(id) < self.nodes.len()
    }

    /// The number of nodes, root included (never zero).
    #[inline]
    #[expect(
        clippy::len_without_is_empty,
        reason = "a tree is born with its root and only grows; an `is_empty` would be `false` by construction"
    )]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Every node with its id, in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (LayerId, &LayerNode)> + '_ {
        self.nodes
            .iter()
            .enumerate()
            .map(|(slot, node)| (id_at(slot), node))
    }

    /// Every id, in insertion order.
    pub(crate) fn ids(&self) -> impl Iterator<Item = LayerId> + '_ {
        (0..self.nodes.len()).map(id_at)
    }
}

#[cfg(test)]
mod tests {
    use flui_types::geometry::{Size, px};

    use super::*;
    use crate::{LeaderLayer, OffsetLayer, PictureLayer};

    fn offset() -> Layer {
        Layer::from(OffsetLayer::zero())
    }

    #[test]
    fn ids_are_one_based_and_dense() {
        let mut tree = LayerTree::new(offset());
        let root = tree.root();
        let a = tree.push_child(root, offset());
        let b = tree.push_child(root, offset());
        assert_eq!(root.get(), 1);
        assert_eq!(a.get(), 2);
        assert_eq!(b.get(), 3);
        assert_eq!(tree.len(), 3);
        assert!(tree.contains(b));
        assert!(!tree.contains(LayerId::new(4)));
        assert_eq!(tree.ids().collect::<Vec<_>>(), vec![root, a, b]);
    }

    #[test]
    fn push_child_links_both_sides_in_paint_order() {
        let mut tree = LayerTree::new(offset());
        let root = tree.root();
        let a = tree.push_child(root, offset());
        let b = tree.push_child(root, Layer::from(PictureLayer::default()));
        let c = tree.push_child(a, offset());

        assert_eq!(tree.root(), root);
        assert_eq!(tree.parent(root), None);
        assert_eq!(tree.parent(a), Some(root));
        assert_eq!(tree.parent(c), Some(a));
        assert_eq!(tree.children(root), Some(&[a, b][..]));
        assert_eq!(tree.children(a), Some(&[c][..]));
        assert_eq!(tree.children(b), Some(&[][..]));
        assert!(matches!(tree.get_layer(b), Some(Layer::Picture(_))));
    }

    #[test]
    fn a_default_tree_is_one_offset_root() {
        let tree = LayerTree::default();
        assert_eq!(tree.len(), 1);
        assert!(matches!(
            tree.get_layer(tree.root()),
            Some(Layer::Offset(_))
        ));
        assert_eq!(tree.parent(tree.root()), None);
        assert_eq!(tree.children(tree.root()), Some(&[][..]));
        assert!(tree.get(LayerId::new(2)).is_none());
        assert_eq!(tree.iter().count(), 1);
    }

    #[test]
    fn leaders_are_indexed_at_insertion() {
        let link = LayerLink::new();
        let other = LayerLink::new();
        let mut tree = LayerTree::new(offset());
        let root = tree.root();
        let leader = tree.push_child(
            root,
            Layer::from(LeaderLayer::new(link, Size::new(px(10.0), px(10.0)))),
        );
        assert_eq!(tree.leader(link), Some(leader));
        assert_eq!(tree.leader(other), None);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn rejected_duplicate_leader_preserves_the_index() {
        let link = LayerLink::new();
        let leader = Layer::from(LeaderLayer::new(link, Size::new(px(10.0), px(10.0))));
        let mut tree = LayerTree::new(leader.clone());
        let root = tree.root();
        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tree.push_child(root, leader)
        }));

        assert!(rejected.is_err());
        assert_eq!(tree.leader(link), Some(root));
        assert_eq!(tree.len(), 1);
        assert_eq!(tree.children(root), Some(&[][..]));
        let child = tree.push_child(root, offset());
        assert_eq!(tree.parent(child), Some(root));
        assert_eq!(tree.leader(link), Some(root));
    }

    #[test]
    fn render_id_stamp_survives_insertion() {
        let render_id = RenderId::new(7);
        let mut tree = LayerTree::new(LayerNode::new(offset()).with_render_id(render_id));
        let root = tree.root();
        let leaf = tree.push_child(root, offset());
        assert_eq!(
            tree.get(root).and_then(LayerNode::render_id),
            Some(render_id)
        );
        assert_eq!(tree.get(leaf).and_then(LayerNode::render_id), None);
    }

    #[test]
    #[should_panic(expected = "parent is not a node of this tree")]
    fn push_child_under_an_unknown_parent_is_a_bug() {
        let mut tree = LayerTree::new(offset());
        let _ = tree.push_child(LayerId::new(2), offset());
    }
}
