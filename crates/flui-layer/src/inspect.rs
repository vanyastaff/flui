//! The layer tree as an [`InspectTree`] (ADR-0242), and each layer's own fields.
//!
//! A node is named by its [`LayerId`] and the tree's [`scene`](LayerTree::scene) (`l4.37`): ids
//! are minted per frame, so a layer of the next frame's tree is a different node even when it
//! paints the same thing, and an id kept from an earlier scene is gone here. A layer that a
//! repaint boundary originated links to that boundary's render object. Its rect is the rectangle
//! the layer defines in its own coordinates, for the variants that define one. Every variant
//! reports the parameters that change what it paints: a typed value where the inspect model has
//! one, and otherwise (a filter, shader, annotation, or a clip shape that is not a rectangle) its
//! `Debug` text. A clip layer also reports how it rasterises its edge, and a picture a fingerprint
//! of its commands next to their count, so a changed drawing reads apart. A backdrop filter names
//! its group by a number its siblings in the group share, and an annotated region its payload by
//! the same kind of number, since the payload is type-erased.

use core::fmt::{self, Write as _};
use flui_foundation::LayerId;
use flui_foundation::inspect::{
    Fields, Inspect, InspectId, InspectTree, Level, Link, Links, NodeView, SceneLayer, Unit, Value,
};
use std::hash::{DefaultHasher, Hasher};
use std::sync::Arc;

use flui_painting::DisplayList;
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
                out.value("blend", Value::Debug(&layer.blend()));
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::ClipRect(layer) => {
                out.field("clip_rect", &layer.clip_rect())
                    .unit(Unit::LogicalPx);
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::ClipRRect(layer) => {
                out.value("clip_rrect", Value::Debug(layer.clip_rrect()));
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::ClipPath(layer) => {
                out.value("clip_path", Value::Debug(layer.clip_path()));
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::ClipSuperellipse(layer) => {
                out.value("clip_superellipse", Value::Debug(layer.clip_superellipse()));
                out.value("clip_behavior", clip_value(layer.clip_behavior()));
            }
            Layer::Picture(layer) => {
                out.field("commands", &layer.picture().len());
                out.field("content", &fingerprint(layer.picture()))
                    .level(Level::Detail);
            }
            Layer::Canvas(layer) => {
                out.field("commands", &layer.display_list().len());
                out.field("content", &fingerprint(layer.display_list()))
                    .level(Level::Detail);
            }
            Layer::Texture(layer) => {
                out.value("texture_id", Value::Debug(&layer.texture_id()));
                out.field("opacity", &layer.opacity()).unit(Unit::Fraction);
                out.field("frozen", &layer.is_frozen());
                out.value("filter_quality", Value::Debug(&layer.filter_quality()));
            }
            Layer::PlatformView(layer) => {
                out.value("view_id", Value::Debug(&layer.view_id()));
                out.value(
                    "hit_test_behavior",
                    Value::Debug(&layer.hit_test_behavior()),
                );
            }
            Layer::PerformanceOverlay(layer) => {
                out.value("options", Value::Debug(&layer.options()));
                out.field("commands", &layer.readout().len());
                out.field("content", &fingerprint(layer.readout()))
                    .level(Level::Detail);
            }
            Layer::ColorFilter(layer) => {
                out.value("color_filter", Value::Debug(&layer.color_filter()));
            }
            Layer::ImageFilter(layer) => {
                out.value("filter", Value::Debug(layer.filter()));
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::ShaderMask(layer) => {
                out.value("shader", Value::Debug(layer.shader()));
                out.value("blend_mode", Value::Debug(&layer.blend_mode()));
            }
            Layer::BackdropFilter(layer) => {
                out.value("filter", Value::Debug(layer.filter()));
                out.value("blend_mode", Value::Debug(&layer.blend_mode()));
                out.value(
                    "group",
                    layer
                        .group()
                        .map_or(Value::None, |key| Value::Uint(key.token())),
                );
            }
            Layer::Leader(layer) => {
                out.value("link", Value::Debug(&layer.link()));
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::Follower(layer) => {
                out.value("link", Value::Debug(&layer.link()));
                out.field("target_offset", &layer.target_offset())
                    .unit(Unit::LogicalPx);
                out.field("show_when_unlinked", &layer.show_when_unlinked());
                out.value("leader_anchor", Value::Debug(&layer.leader_anchor()));
                out.value("follower_anchor", Value::Debug(&layer.follower_anchor()));
                out.field("size", &layer.size()).unit(Unit::LogicalPx);
            }
            Layer::AnnotatedRegion(layer) => {
                // The payload is erased, so it reads by identity: the same `Arc` reads equal, another
                // one apart, as `find_annotations` would return it.
                out.value(
                    "value",
                    Value::Uint(Arc::as_ptr(layer.value()).addr() as u64),
                );
                out.field("sized_by_parent", &layer.is_sized_by_parent());
            }
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

/// A fingerprint of a display list's commands, so two pictures of one length and bounds that
/// draw differently read apart. It hashes each command's `Debug` text as it is written, keeping
/// none of it; equal within one build of the program, not across builds.
fn fingerprint(list: &DisplayList) -> u64 {
    struct Hashing(DefaultHasher);

    impl fmt::Write for Hashing {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            self.0.write(text.as_bytes());
            Ok(())
        }
    }

    let mut out = Hashing(DefaultHasher::new());
    for command in list {
        write!(out, "{command:?}").expect("BUG: hashing text never fails");
    }
    out.0.finish()
}
