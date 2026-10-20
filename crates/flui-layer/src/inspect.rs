//! The layer tree as an [`InspectTree`] (ADR-0242), and each layer's own fields.
//!
//! A node is named by its [`LayerId`] and the tree's [`scene`](LayerTree::scene) (`l4.37`): ids
//! are minted per frame, so a layer of the next frame's tree is a different node even when it
//! paints the same thing, and an id kept from an earlier scene is gone here. A layer that a
//! repaint boundary originated links to that boundary's render object. Its rect is the rectangle
//! the layer defines in its own coordinates, for the variants that define one; a clip layer adds
//! how it rasterises the edge.

use flui_foundation::LayerId;
use flui_foundation::inspect::{
    Fields, Inspect, InspectId, InspectTree, Link, Links, NodeView, SceneLayer, Unit, Value,
};
use flui_painting::paint::Clip;

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
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::ClipRRect(layer) => {
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::ClipPath(layer) => {
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::ClipSuperellipse(layer) => {
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::Picture(layer) => {
                out.field("commands", &layer.picture().len());
            }
            _ => {}
        }
    }
}

impl LayerTree {
    /// `id` as a layer of this scene.
    fn in_scene(&self, id: LayerId) -> SceneLayer {
        SceneLayer {
            scene: self.scene(),
            id,
        }
    }

    /// The layer `id` names, when it names one of this scene.
    fn layer_id(&self, id: InspectId) -> Option<LayerId> {
        match id.link() {
            Link::Layer(layer) if layer.scene == self.scene() => Some(layer.id),
            _ => None,
        }
    }
}

impl InspectTree for LayerTree {
    type Node = Layer;

    fn root(&self) -> Option<InspectId> {
        Some(self.in_scene(LayerTree::root(self)).into())
    }

    fn node(&self, id: InspectId) -> Option<NodeView<'_, Layer>> {
        let id = self.layer_id(id)?;
        let node = self.get(id)?;
        Some(NodeView {
            object: node.layer(),
            rect: node.layer().bounds(),
            links: Links {
                layer: Some(self.in_scene(id)),
                render: node.render_id(),
                ..Links::default()
            },
        })
    }

    fn children(&self, id: InspectId) -> impl Iterator<Item = InspectId> + '_ {
        self.layer_id(id)
            .and_then(|id| LayerTree::children(self, id))
            .unwrap_or_default()
            .iter()
            .map(|&child| self.in_scene(child).into())
    }
}

/// How a clip layer rasterises its edge, which tells clips of the same shape apart.
fn clip_value(clip: Clip) -> Value<'static> {
    Value::Variant {
        ty: "Clip",
        name: match clip {
            Clip::None => "None",
            Clip::HardEdge => "HardEdge",
            Clip::AntiAlias => "AntiAlias",
            Clip::AntiAliasWithSaveLayer => "AntiAliasWithSaveLayer",
        },
    }
}
