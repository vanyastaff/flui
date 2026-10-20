//! The layer tree as an [`InspectTree`] (ADR-0242), and each layer's own fields.
//!
//! A node is named by its [`LayerId`] (`l4`), which is minted per frame: a layer of the next
//! frame's tree is a different node even when it paints the same thing. A layer that a repaint
//! boundary originated links to that boundary's render object. Its rect is the rectangle the
//! layer defines in its own coordinates, for the variants that define one.

use flui_foundation::LayerId;
use flui_foundation::inspect::{
    Fields, Inspect, InspectId, InspectTree, Link, Links, NodeView, Unit,
};

use crate::{Layer, LayerTree};

impl Inspect for Layer {
    fn kind(&self) -> &'static str {
        self.kind_name()
    }

    fn inspect(&self, out: &mut Fields<'_>) {
        match self {
            Layer::Offset(layer) => {
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::Transform(layer) => {
                out.field("transform", layer.transform());
            }
            Layer::Opacity(layer) => {
                out.field("alpha", &layer.alpha()).unit(Unit::Fraction);
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::ClipRect(layer) => {
                out.field("clip_rect", &layer.clip_rect())
                    .unit(Unit::LogicalPx);
            }
            Layer::Picture(layer) => {
                out.field("commands", &layer.picture().len());
            }
            _ => {}
        }
    }
}

fn layer_id(id: InspectId) -> Option<LayerId> {
    match id.link() {
        Link::Layer(id) => Some(id),
        _ => None,
    }
}

impl InspectTree for LayerTree {
    type Node = Layer;

    fn root(&self) -> Option<InspectId> {
        Some(LayerTree::root(self).into())
    }

    fn node(&self, id: InspectId) -> Option<NodeView<'_, Layer>> {
        let id = layer_id(id)?;
        let node = self.get(id)?;
        Some(NodeView {
            object: node.layer(),
            rect: node.layer().bounds(),
            links: Links {
                layer: Some(id),
                render: node.render_id(),
                ..Links::default()
            },
        })
    }

    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        layer_id(id)
            .and_then(|id| LayerTree::children(self, id))
            .unwrap_or_default()
            .iter()
            .map(|&child| child.into())
    }
}
