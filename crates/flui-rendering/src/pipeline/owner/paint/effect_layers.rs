//! Mapping of recorded effect scopes and clips onto `flui-layer` layers, and the effect entries a
//! node pushed in a content.

use std::sync::Arc;

use flui_foundation::geometry::Offset;
use flui_layer::{
    AnnotatedRegionLayer, BackdropFilterLayer, ClipPathLayer, ClipRRectLayer, ClipRectLayer,
    ClipSuperellipseLayer, Effect, EntryIndex, LeaderLayer, OpacityLayer, ShaderMaskLayer,
    TransformLayer,
};
use smallvec::SmallVec;

use crate::{
    context::FragmentScope,
    traits::{PaintClip, PaintEffects, resolve_path_clip},
};

/// The effect entries one node pushed in a content, and the origin it
/// painted at.
#[derive(Clone, Debug)]
pub(super) struct EffectSlots {
    pub(super) indices: SmallVec<[EntryIndex; 3]>,
    pub(super) origin: Offset,
}

/// Adds to `layers` the layers a node pushes for its OWN effects, in push order (outermost
/// first: opacity, clip, transform), built from the single
/// [`PaintEffects`] value the paint walk reads off the node.
///
/// One value in, two callers, deliberately: the paint walk pushes these
/// while painting, and the composited-layer-update patch arm rebuilds them
/// without painting. Two copies of this construction would drift, and the
/// drift would be a wrong-looking layer on a frame no test paints.
///
/// `origin` matters to the clip and the transform, not the opacity: both a
/// clip shape and a transform matrix are reported in the node's LOCAL
/// coordinates while every run inside the layer space carries the
/// accumulated origin, so both are conjugated by it ([`clip_layer`] for the
/// clip, [`conjugate`] for the transform). The opacity layer is
/// origin-independent (always `Offset::ZERO`), which is why an alpha-only
/// update is correct no matter where the node sits.
pub(super) fn own_effect_layers(
    effects: PaintEffects,
    origin: Offset,
    layers: &mut impl Extend<Effect>,
) {
    if let Some(opacity) = effects.opacity {
        layers.extend(Some(Effect::from(OpacityLayer::with_offset(
            opacity.alpha(),
            Offset::ZERO,
        ))));
    }
    if let Some(clip) = effects.clip {
        layers.extend(Some(clip_layer(clip, origin)));
    }
    if let Some(matrix) = effects.transform {
        // Same math and same reason as the per-child `PushTransform` fragment
        // op (RenderFlow and friends).
        layers.extend(Some(Effect::from(TransformLayer::new(conjugate(
            matrix, origin,
        )))));
    }
}

/// Conjugates `matrix` so it pivots around this layer's local `origin`
/// rather than the layer tree's own (0, 0).
///
/// Both callers report a transform in LOCAL coordinates while every run
/// they bracket carries the accumulated `origin` baked into its canvas
/// transform: the per-node [`RenderObject::paint_effects`](crate::traits::RenderObject::paint_effects)
/// value's `transform` field (one transform for the whole node, applied
/// here) and the per-child [`FragmentOp::PushTransform`] op (`RenderFlow`
/// and any other Variable-arity node giving each child its own paint-time
/// transform). The result is `T(offset)·M·T(−offset)`.
pub(super) fn conjugate(
    matrix: flui_foundation::geometry::Matrix4,
    origin: Offset,
) -> flui_foundation::geometry::Matrix4 {
    if origin == Offset::ZERO {
        matrix
    } else {
        let (dx, dy) = (origin.dx, origin.dy);
        flui_foundation::geometry::Matrix4::translation(dx, dy, 0.0)
            * matrix
            * flui_foundation::geometry::Matrix4::translation(-dx, -dy, 0.0)
    }
}

/// A backdrop filter recorded in a node's local space, moved into the space
/// of a layer that draws the node at `origin`: a transform filter's focal
/// point stays where the node put it. A move that leaves the transform
/// non-finite keeps the filter as recorded, so nothing non-finite reaches
/// the scene.
fn backdrop_filter_at(
    filter: flui_painting::paint::ImageFilter,
    origin: Offset,
) -> flui_painting::paint::ImageFilter {
    if origin == Offset::ZERO {
        return filter;
    }
    match filter.try_translated(origin) {
        Ok(moved) => moved,
        Err(error) => {
            tracing::warn!(?origin, %error, "a backdrop filter's transform could not follow its node");
            filter
        }
    }
}

/// Maps a recorded effect-layer scope onto its `flui-layer` layer.
///
/// Scope shapes/bounds are recorded in the node's LOCAL coordinates, while
/// the runs they bracket carry the accumulated `origin` baked into their
/// canvas transforms — so every variant is shifted by `origin` here
/// (a clip rect is `clipRect.shift(offset)`; a shader mask's rect and a
/// backdrop filter's bounds follow the same `offset & size` convention),
/// or a scope away from the
/// parent origin would apply at the layer's (0,0) instead of the node's
/// position.
///
/// Always a real layer today; lowering non-composited clips back into
/// canvas clips inside a picture is a composer-side optimization
/// gated on the `needs_compositing` bits — correctness is identical
/// either way, so the recording API does not expose the choice.
pub(super) fn scope_layer(scope: FragmentScope, origin: Offset) -> Effect {
    match scope {
        FragmentScope::Opacity(opacity) => {
            Effect::from(OpacityLayer::with_offset(opacity.alpha(), Offset::ZERO))
        }
        FragmentScope::Clip(clip) => clip_layer(clip, origin),
        FragmentScope::ShaderMask {
            shader,
            blend_mode,
            bounds,
        } => Effect::from(ShaderMaskLayer::new(
            shader,
            blend_mode,
            bounds.translate_offset(origin),
        )),
        FragmentScope::BackdropFilter {
            filter,
            blend_mode,
            bounds,
            group,
        } => Effect::from(
            BackdropFilterLayer::new(
                backdrop_filter_at(filter, origin),
                blend_mode,
                bounds.translate_offset(origin),
            )
            .with_group(group),
        ),
        FragmentScope::AnnotatedRegion { value, bounds } => Effect::from(
            AnnotatedRegionLayer::from_value(bounds.translate_offset(origin), value),
        ),
        // Oracle `LeaderLayer(link: link, offset: offset)` — `offset` is
        // this node's own accumulated position, exactly what `origin`
        // already is at this call site (`:270`). Unlike the clip/mask
        // variants above, `size` is NOT shifted by `origin` — it is a
        // pure dimension, not a position.
        FragmentScope::Leader { link, size } => {
            Effect::from(LeaderLayer::with_offset(link, size, origin))
        }
        // `Layer::Follower` carries no resolved transform at all — its
        // anchors and offsets are inputs to the composite-time
        // `SceneVersion::follower_transform`. `target_offset` is
        // recorded as-authored (not origin-shifted), like `size`.
        FragmentScope::Follower(follower) => Effect::from(follower),
    }
}

/// Maps a recorded [`PaintClip`] onto its `flui-layer` clip layer.
///
/// Shifted by `origin` for the same reason every [`scope_layer`] variant
/// is (see its doc): `Rect`/`RRect` translate their shape directly.
/// `Path` is the one case worth spelling out — at `origin == Offset::ZERO`
/// (the node sits at the layer-space origin, directly under its boundary)
/// the already-owned `Arc<Path>` moves through untouched, no clone and no
/// copy of the command buffer; only a non-zero origin pays for a
/// translated path in a freshly allocated `Arc`. `PathTarget` resolves
/// through [`resolve_path_clip`] first (never by the producer, and inside
/// the walk that calls this function), then follows the same
/// translate-then-box shape as `Path` — the one difference is that its
/// `Arc::new` is unavoidable either way, since the resolver always hands
/// back a freshly built owned `Path`, not a pre-existing `Arc`.
fn clip_layer(clip: PaintClip, origin: Offset) -> Effect {
    match clip {
        PaintClip::Rect { rect, behavior } => {
            Effect::from(ClipRectLayer::new(rect.translate_offset(origin), behavior))
        }
        PaintClip::RRect { rrect, behavior } => Effect::from(ClipRRectLayer::new(
            rrect.translate_offset(origin),
            behavior,
        )),
        PaintClip::RSuperellipse {
            rsuperellipse,
            behavior,
        } => Effect::from(ClipSuperellipseLayer::new(
            rsuperellipse.translate_offset(origin),
            behavior,
        )),
        PaintClip::Path { path, behavior } => {
            let path = if origin == Offset::ZERO {
                path
            } else {
                Arc::new(path.translate(origin))
            };
            Effect::from(ClipPathLayer::new(path, behavior))
        }
        PaintClip::PathTarget {
            target,
            size,
            behavior,
        } => {
            // The path arm above shares its already-owned `Arc<Path>` for
            // free at zero origin; there is no pre-existing `Arc` to share
            // here — `resolve_path_clip` hands back a freshly built owned
            // `Path` every time, so the single `Arc::new` below is the one
            // allocation this arm ever pays, made AFTER the translate so a
            // non-zero origin never boxes a path it is about to discard.
            let path = resolve_path_clip(target, size);
            let path = if origin == Offset::ZERO {
                path
            } else {
                path.translate(origin)
            };
            Effect::from(ClipPathLayer::new(Arc::new(path), behavior))
        }
    }
}
