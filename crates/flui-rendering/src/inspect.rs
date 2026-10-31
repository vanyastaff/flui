//! The render tree as an [`InspectTree`] (ADR-0242).
//!
//! [`PipelineOwner::inspect`] reads the committed tree: what each node was laid out with
//! (protocol, constraints, size or sliver geometry, offset), its dirty flags, and the semantics
//! node it contributes, followed by the render object's own properties. A node is named by its
//! generational [`RenderId`] (`r12`, `r12.2` after the slot is reused), so an id from before a
//! removal answers [`SnapshotError::Gone`](flui_foundation::inspect::SnapshotError::Gone)
//! rather than another node.
//!
//! The render object's own fields are its `Inspect` fields, written after the tree's facts.

use flui_foundation::geometry::{Offset, Rect};
use flui_foundation::inspect::{
    Fields, Inspect, InspectId, InspectTree, Level, Link, Links, NodeView, Unit, Value,
};
use flui_foundation::{RenderId, SemanticsId};
use rustc_hash::FxHashMap;

use crate::pipeline::{PipelineOwner, PipelinePhase};
use crate::storage::{RenderNode, RenderTree};

impl Inspect for RenderNode {
    fn kind(&self) -> &'static str {
        match self {
            Self::Box(entry) => entry.render_object().kind(),
            Self::Sliver(entry) => entry.render_object().kind(),
        }
    }

    fn kind_args(&self, args: &mut dyn FnMut(&'static str)) {
        match self {
            Self::Box(entry) => entry.render_object().kind_args(args),
            Self::Sliver(entry) => entry.render_object().kind_args(args),
        }
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        match self {
            Self::Box(entry) => entry.render_object().inspect(out),
            Self::Sliver(entry) => entry.render_object().inspect(out),
        }
    }
}

/// A pipeline's render tree, read as an [`InspectTree`].
#[derive(Debug)]
pub struct RenderInspect<'a> {
    tree: &'a RenderTree,
    root: Option<RenderId>,
    /// The semantics node each render object contributes, from the committed semantics tree.
    semantics: FxHashMap<RenderId, SemanticsId>,
}

impl<Phase: PipelinePhase> PipelineOwner<Phase> {
    /// The render tree from [`root_id`](Self::root_id), for a typed snapshot
    /// ([`flui_foundation::inspect::snapshot`]).
    ///
    /// Nodes link to the semantics nodes they contribute while semantics is enabled.
    #[must_use]
    pub fn inspect(&self) -> RenderInspect<'_> {
        let mut semantics = FxHashMap::default();
        if let Some(owner) = self.semantics_owner() {
            for (id, node) in owner.tree().iter() {
                if let Some(render) = node.source_render_id() {
                    semantics.entry(render).or_insert(id);
                }
            }
        }
        RenderInspect {
            tree: self.render_tree(),
            root: self.root_id(),
            semantics,
        }
    }
}

fn render_id(id: InspectId) -> Option<RenderId> {
    match id.link() {
        Link::Render(id) => Some(id),
        _ => None,
    }
}

impl InspectTree for RenderInspect<'_> {
    type Node = RenderNode;

    fn root(&self) -> Option<InspectId> {
        self.root.map(InspectId::from)
    }

    fn node(&self, id: InspectId) -> Option<NodeView<'_, RenderNode>> {
        let id = render_id(id)?;
        let node = self.tree.get(id)?;
        let rect = node.as_box().and_then(|entry| {
            let size = entry.state().geometry()?;
            let offset = node.offset();
            Some(Rect::from_ltwh(
                offset.dx,
                offset.dy,
                size.width,
                size.height,
            ))
        });
        Some(NodeView {
            object: node,
            rect,
            links: Links {
                render: Some(id),
                semantics: self.semantics.get(&id).copied(),
                ..Links::default()
            },
        })
    }

    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        render_id(id)
            .and_then(|id| self.tree.get(id))
            .map(RenderNode::children)
            .unwrap_or_default()
            .iter()
            .map(|&child| child.into())
    }

    fn facts(&self, id: InspectId, out: &mut Fields<'_>) {
        let Some(node) = render_id(id).and_then(|id| self.tree.get(id)) else {
            return;
        };
        out.field("protocol", node.protocol_name()).default("Box");
        match node {
            RenderNode::Box(entry) => {
                if let Some(constraints) = entry.state().constraints() {
                    // An unbounded maximum is infinity, which is what it is.
                    out.field("constraints_min_width", &constraints.min_width)
                        .unit(Unit::LogicalPx)
                        .level(Level::Detail);
                    out.field("constraints_min_height", &constraints.min_height)
                        .unit(Unit::LogicalPx)
                        .level(Level::Detail);
                    out.field("constraints_max_width", &constraints.max_width)
                        .unit(Unit::LogicalPx)
                        .level(Level::Detail);
                    out.field("constraints_max_height", &constraints.max_height)
                        .unit(Unit::LogicalPx)
                        .level(Level::Detail);
                }
                if let Some(size) = entry.state().geometry() {
                    out.field("size", &size).unit(Unit::LogicalPx);
                }
            }
            RenderNode::Sliver(entry) => {
                // `SliverConstraints` and `SliverGeometry` are twelve-field records with no single
                // `Value`; a typed split would rename the `geometry` fact that sliver assertions
                // read, so both stay described by their `Debug`.
                if let Some(constraints) = entry.state().constraints() {
                    out.value("constraints", Value::Debug(constraints))
                        .level(Level::Detail);
                }
                if let Some(geometry) = entry.state().geometry() {
                    out.value("geometry", Value::Debug(&geometry));
                }
            }
        }
        out.field("offset", &node.offset())
            .unit(Unit::LogicalPx)
            .default(&Offset::ZERO);
        for (name, set) in [
            ("needs_layout", node.needs_layout()),
            ("needs_paint", node.needs_paint()),
            ("needs_semantics", node.needs_semantics()),
        ] {
            out.field(name, &set).level(Level::Detail).default(&false);
        }
    }
}
