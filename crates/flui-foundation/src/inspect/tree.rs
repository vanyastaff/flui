//! Identity and structure, which belong to the tree and not to the object.

use core::fmt;
use core::num::NonZeroU64;

use crate::geometry::Rect;
use crate::id::{ElementId, LayerId, RenderId, SemanticsId};

use super::fields::{Fields, Inspect};

/// A layer of one painted scene.
///
/// A [`LayerId`] is minted per frame from the first slot, so the same id names an unrelated
/// layer in the next scene. The scene number tells the two apart, as a generation does for a
/// reused slot of a generational id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneLayer {
    /// The scene the layer belongs to, counted from 1 by the tree's producer.
    pub scene: NonZeroU64,
    /// The layer within that scene.
    pub id: LayerId,
}

/// A typed reference to a node of one of the trees.
///
/// Its text form is the tree's prefix and the id's slot: `v3` (element), `r12` (render), `l4`
/// (layer), `s7` (semantics). A generational id whose generation is past the first adds it
/// (`r12.2`), so a reused slot never reads as the node it replaced; a layer adds its scene the same
/// way (`l4.37`).
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
            Self::Element(id) => (u64::from(id.index()), u64::from(id.generation().get())),
            Self::Render(id) => (u64::from(id.index()), u64::from(id.generation().get())),
            Self::Layer(layer) => (layer.id.get() as u64, layer.scene.get()),
            Self::Semantics(id) => (id.get() as u64, 1),
        };
        write!(f, "{}{slot}", self.prefix())?;
        if generation > 1 {
            write!(f, ".{generation}")?;
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
