//! Each layer's own fields, as the retained scene's [`InspectTree`] reports an effect
//! (ADR-0242; the tree is `scene::inspect`).
//!
//! Its rect is the rectangle the layer defines in its own coordinates, for the variants that
//! define one. Every variant reports the parameters that change what it paints: a typed value
//! where the inspect model has one, and otherwise (a filter, shader, annotation, or a clip shape
//! that is not a rectangle) its `Debug` text. A clip layer also reports how it rasterises its
//! edge, and a picture a fingerprint of its commands next to their count, so a changed drawing
//! reads apart. A backdrop filter names its group as a [`Shared`] object, `BackdropGroup#1`,
//! numbered in the order the snapshot meets the groups, and an annotated region its payload the
//! same way next to the payload's `Debug` text.
//!
//! [`InspectTree`]: flui_foundation::inspect::InspectTree

use core::fmt::{self, Write as _};
use flui_foundation::inspect::{Fields, Inspect, Level, Shared, Unit, Untyped, Value};
use std::hash::{DefaultHasher, Hasher};

use flui_painting::DisplayList;

use crate::Layer;

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
                out.field("blend", &layer.blend());
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::ClipRect(layer) => {
                out.field("clip_rect", &layer.clip_rect())
                    .unit(Unit::LogicalPx);
                out.field("clip_behavior", &layer.clip_behavior());
            }
            Layer::ClipRRect(layer) => {
                #[expect(deprecated, reason = "a rounded rectangle has no typed value")]
                out.value("clip_rrect", Value::Debug(Untyped::new(layer.clip_rrect())));
                out.field("clip_behavior", &layer.clip_behavior());
            }
            Layer::ClipPath(layer) => {
                #[expect(deprecated, reason = "a path has no typed value")]
                out.value("clip_path", Value::Debug(Untyped::new(layer.clip_path())));
                out.field("clip_behavior", &layer.clip_behavior());
            }
            Layer::ClipSuperellipse(layer) => {
                #[expect(deprecated, reason = "a superellipse has no typed value")]
                out.value(
                    "clip_superellipse",
                    Value::Debug(Untyped::new(layer.clip_superellipse())),
                );
                out.field("clip_behavior", &layer.clip_behavior());
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
                out.field("texture_id", &layer.texture_id().get());
                out.field("opacity", &layer.opacity()).unit(Unit::Fraction);
                out.field("frozen", &layer.is_frozen());
                out.field("filter_quality", &layer.filter_quality());
            }
            Layer::PlatformView(layer) => {
                out.field("view_id", &layer.view_id().value());
                out.field("hit_test_behavior", &layer.hit_test_behavior());
            }
            Layer::PerformanceOverlay(layer) => {
                out.field("options", &layer.options().bits());
                out.field("commands", &layer.readout().len());
                out.field("content", &fingerprint(layer.readout()))
                    .level(Level::Detail);
            }
            Layer::ColorFilter(layer) => {
                #[expect(deprecated, reason = "ColorFilter is a composite with no typed value")]
                out.value(
                    "color_filter",
                    Value::Debug(Untyped::new(&layer.color_filter())),
                );
            }
            Layer::ImageFilter(layer) => {
                #[expect(deprecated, reason = "ImageFilter is a composite with no typed value")]
                out.value("filter", Value::Debug(Untyped::new(layer.filter())));
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::ShaderMask(layer) => {
                #[expect(deprecated, reason = "a shader is a composite with no typed value")]
                out.value("shader", Value::Debug(Untyped::new(layer.shader())));
                out.field("blend_mode", &layer.blend_mode());
            }
            Layer::BackdropFilter(layer) => {
                #[expect(deprecated, reason = "ImageFilter is a composite with no typed value")]
                out.value("filter", Value::Debug(Untyped::new(layer.filter())));
                out.field("blend_mode", &layer.blend_mode());
                out.value(
                    "group",
                    layer
                        .group()
                        .map_or(Value::None, |key| Value::Shared(key.shared())),
                );
            }
            Layer::Leader(layer) => {
                out.field("link", &layer.link().id());
                out.field("offset", &layer.offset()).unit(Unit::LogicalPx);
            }
            Layer::Follower(layer) => {
                out.field("link", &layer.link().id());
                out.field("target_offset", &layer.target_offset())
                    .unit(Unit::LogicalPx);
                out.field("show_when_unlinked", &layer.show_when_unlinked());
                out.field("leader_anchor", &layer.leader_anchor())
                    .unit(Unit::Fraction);
                out.field("follower_anchor", &layer.follower_anchor())
                    .unit(Unit::Fraction);
                out.field("leader_clip", &layer.leader_clip());
                out.field("size", &layer.size()).unit(Unit::LogicalPx);
            }
            Layer::AnnotatedRegion(layer) => {
                // The payload is type-erased: it reports its `Debug` text, and which value it is
                // by a number that regions sharing one `Arc` share.
                let value = layer.value();
                #[expect(
                    deprecated,
                    reason = "the region payload is type-erased: only its `Debug` text is known"
                )]
                out.value("value", Value::Debug(Untyped::new(value)));
                out.value(
                    "instance",
                    Value::Shared(Shared::arc(value.type_name(), value.erased())),
                );
                out.field("sized_by_parent", &layer.is_sized_by_parent());
            }
        }
    }
}

/// A fingerprint of a display list's commands, so two pictures of one length and bounds that
/// draw differently read apart. It hashes each command's `Debug` text as it is written, keeping
/// none of it; equal within one build of the program, not across builds.
pub(crate) fn fingerprint(list: &DisplayList) -> u64 {
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
