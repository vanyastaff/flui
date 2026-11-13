//! Identity and structure, which belong to the tree and not to the object.

use core::fmt;
use core::num::NonZeroU64;

use crate::generation::Generation;
use crate::geometry::Rect;
use crate::id::{ElementId, RenderId, SemanticsId};

use super::fields::{Fields, Inspect};

/// A boundary of one retained scene, or one entry of what it paints.
///
/// A boundary keeps its node across frames while it stays in the scene, so a repaint elsewhere
/// leaves its id alone; a retired node's slot is reissued under the next generation, and an owner
/// that starts over takes a new lineage, so a kept id never names an unrelated node. An entry is
/// counted within the boundary's current content, so an id kept from content the boundary has
/// since repainted names what now sits at that position, or nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneLayer {
    /// The lineage of the scene owner the node belongs to.
    pub scene: NonZeroU64,
    /// The node's slot in its scene, counted from 0.
    pub slot: u32,
    /// The generation of that slot the node holds.
    pub generation: Generation,
    /// The entry of the node's content, counted from 0 in paint order; `None` for the node.
    pub entry: Option<u32>,
}

/// A typed reference to a node of one of the trees.
///
/// Its text form is the tree's prefix and the id's slot: `v3` (element), `r12` (render), `l4`
/// (layer), `s7` (semantics). A generational id whose generation is past the first adds it
/// (`r12.2`), so a reused slot never reads as the node it replaced. A layer adds its entry after a
/// colon and its scene after an at sign: `l4.2:3@7` is entry 3 of node `l4.2@7`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Link {
    /// An element (the widget tree).
    Element(ElementId),
    /// A render object.
    Render(RenderId),
    /// A layer.
    Layer(SceneLayer),
    /// A semantics node.
    Semantics(SemanticsId),
}

impl Link {
    /// The tree's prefix: `v`, `r`, `l` or `s`.
    #[must_use]
    pub const fn prefix(self) -> char {
        match self {
            Self::Element(_) => 'v',
            Self::Render(_) => 'r',
            Self::Layer(_) => 'l',
            Self::Semantics(_) => 's',
        }
    }
}

impl fmt::Display for Link {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (slot, generation) = match *self {
            Self::Element(id) => (u64::from(id.index()), u64::from(id.generation().as_u32())),
            Self::Render(id) => (u64::from(id.index()), u64::from(id.generation().as_u32())),
            Self::Layer(layer) => (u64::from(layer.slot), u64::from(layer.generation.as_u32())),
            Self::Semantics(id) => (id.get() as u64, 1),
        };
        write!(f, "{}{slot}", self.prefix())?;
        if generation > 1 {
            write!(f, ".{generation}")?;
        }
        if let Self::Layer(layer) = self {
            if let Some(entry) = layer.entry {
                write!(f, ":{entry}")?;
            }
            write!(f, "@{}", layer.scene)?;
        }
        Ok(())
    }
}

/// A node's identity within the tree that describes it, assigned by the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InspectId(Link);

impl InspectId {
    /// The id as a link, as another tree's node refers to this one.
    #[must_use]
    pub const fn link(self) -> Link {
        self.0
    }
}

impl From<Link> for InspectId {
    fn from(link: Link) -> Self {
        Self(link)
    }
}

impl From<ElementId> for InspectId {
    fn from(id: ElementId) -> Self {
        Self(Link::Element(id))
    }
}

impl From<RenderId> for InspectId {
    fn from(id: RenderId) -> Self {
        Self(Link::Render(id))
    }
}

impl From<SceneLayer> for InspectId {
    fn from(layer: SceneLayer) -> Self {
        Self(Link::Layer(layer))
    }
}

impl From<SemanticsId> for InspectId {
    fn from(id: SemanticsId) -> Self {
        Self(Link::Semantics(id))
    }
}

impl fmt::Display for InspectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A node's links to its counterparts in the other trees.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Links {
    /// The element that built it.
    pub element: Option<ElementId>,
    /// Its render object.
    pub render: Option<RenderId>,
    /// Its layer.
    pub layer: Option<SceneLayer>,
    /// Its semantics node.
    pub semantics: Option<SemanticsId>,
}

impl Links {
    /// Whether one of the links is `link`.
    #[must_use]
    pub fn contains(&self, link: Link) -> bool {
        match link {
            Link::Element(id) => self.element == Some(id),
            Link::Render(id) => self.render == Some(id),
            Link::Layer(layer) => self.layer == Some(layer),
            Link::Semantics(id) => self.semantics == Some(id),
        }
    }

    /// The links that are set, element first.
    pub fn iter(&self) -> impl Iterator<Item = Link> {
        [
            self.element.map(Link::Element),
            self.render.map(Link::Render),
            self.layer.map(Link::Layer),
            self.semantics.map(Link::Semantics),
        ]
        .into_iter()
        .flatten()
    }
}

/// One node as its tree shows it: the object, and what the tree knows about it.
#[derive(Debug)]
pub struct NodeView<'a, N: ?Sized> {
    /// The object, which describes its own fields.
    pub object: &'a N,
    /// Its committed bounds in its parent's logical coordinates, when the tree lays it out.
    pub rect: Option<Rect>,
    /// Its counterparts in the other trees.
    pub links: Links,
}

/// A tree that can be inspected: the element, render and layer trees implement it.
///
/// Children come from the tree, so a walk can stop at a depth or a node count without
/// building anything for the nodes it skips.
pub trait InspectTree {
    /// What the tree's nodes are.
    type Node: Inspect + ?Sized;

    /// The root, or `None` for an empty tree.
    fn root(&self) -> Option<InspectId>;

    /// The node `id`, or `None` when the tree has no such node.
    fn node(&self, id: InspectId) -> Option<NodeView<'_, Self::Node>>;

    /// The children of `id`, in order.
    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_;

    /// What the tree knows about `id` that the object does not: lifecycle, dirty flags.
    ///
    /// A snapshot keeps these in [`SnapshotNode::facts`](super::SnapshotNode::facts), apart
    /// from the object's own fields, so a fact may share a name with one of them.
    fn facts(&self, id: InspectId, out: &mut Fields<'_>) {
        let _ = (id, out);
    }
}
