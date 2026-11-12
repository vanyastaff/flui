# ADR-0099: A save layer composites its whole region with its recorded blend mode

- **Status:** Accepted (2026-11-03)
- **Revised:** 2026-11-03 — "Limits" states that the engine's three limits are gone.
- **Superseded-by:** [ADR-0102](ADR-0102-immutable-geometric-clip-composition.md) for §1–§4
  (§6 of ADR-0102 restates the region, the whole-region blend, the damage and the shader-mask
  rule; this record keeps its text as the reasoning).
- **Date:** 2026-09-30
- **Related:** [ADR-0057](ADR-0057-coverage-correct-blending-is-capability-gated.md) (coverage-correct blending,
  shapes only), [ADR-0087](ADR-0087-raster-contract-and-cpu-backend.md) (the raster contract
  a CPU backend must match)

ADR-0102 retains whole-region compositing while replacing single-slot clip and
doubled-edge limitations with immutable expression ownership. It also supersedes
ADR-0057's former permission for incorrect featureless direct destructive AA;
those direct draws take ADR-0103's portable path, or return `UnsupportedCoverageBlend`
on a view-only target.

## Context

Three crates share one answer to "which pixels does a layer change":

- **`flui-painting`** records `Canvas::save_layer(bounds, paint)` and reports each op's damage
  extent; a bounded save layer's extent is its bounds mapped through the current transform.
- **`flui-layer`**'s scene reports an effect's extent in the footprint of the patch that changes it
  (ADR-0304 §4); an opacity layer whose blend changes pixels under a transparent source reaches
  everywhere the clips above it allow.
- **`flui-engine`** composites the layer.

The engine used to composite every save layer and opacity layer `SrcOver`-like, dropping texels
below 1% alpha, so a `Src` layer replaced only under its content, an empty `Clear` layer did
nothing, and a bounded layer composited over its unmapped local bounds. The damage producers
already assumed the wider region, so a partial frame and the composite disagreed on what a layer
covers.

## Decision

1. **Region.** A save layer's region is its `bounds`, given in the local space of the transform
   current at `save_layer`, mapped through that transform and cut by the clip in force there
   (clip rects, a rounded clip, a partial frame's damage scissor). With no `bounds`, or under a
   projective transform, the region is the clip. An opacity layer's region is its clip.
2. **Mode over the whole region.** On restore the layer composites its whole region with the
   blend mode it records, the pixels its content left transparent included. A mode whose
   `BlendMode::keeps_destination_under_transparent_source()` is false (`Clear`, `Src`, `SrcIn`,
   `DstIn`, `SrcOut`, `DstATop`, `Modulate`) changes every pixel of the region, and an empty
   layer in such a mode still composites. A layer never changes a pixel its clip excludes,
   and the region bounds its content for every mode, not only the destination-replacing
   ones: nothing the layer holds shows outside its region, under a rotation or skew the
   exact quad rather than its bounding box.
3. **Damage.** A damage producer reports at least the region of (1) for a layer in a mode of
   (2): `DrawOp`'s extent for a bounded save layer is its mapped bounds, an unbounded one is
   unbounded, and a scene patch that changes such an opacity layer damages everywhere the clips
   above it allow.
4. **Shader masks are not save layers.** A shader mask's blend mode combines its shader with its
   child. Its masked result composites `SrcOver`, under the clip in force where the mask was
   recorded. Applied at the composite, the default `Modulate` (or `SrcIn`, the gradient-text
   mode) would multiply the child by the backdrop and erase the backdrop around it.

## Why

- `save_layer(bounds, Src)` reads as "this region becomes exactly the layer". Replacing only
  under the content makes `Src` depend on how far the content reaches and makes an empty
  `Clear` layer a no-op.
- A paint-level mode already behaves this way: a `Src` rect with a partly transparent shader
  replaces every pixel of the rect. Restoring a layer is a draw of its buffer over its region.
- Bounds in local space move with the content they bound; unmapped bounds put a translated or
  scaled layer's composite over the wrong rectangle.
- (3) is the extent the producers reported before the engine honoured (1) and (2), so the
  partial-frame path needed no change.

## Limits

The `wgpu` engine has none left in (1)–(3). A layer's bounds join its composite clip expression
under their captured transform (`crates/flui-engine/src/painter/layer.rs`), so a bounded layer
always composites and its region is the exact quad under a rotation or skew inside a rounded
clip, and the mix in the anti-aliased edge of a clip is ADR-0102 §5's. Engine mapping decision
19 in `crates/flui-engine/ARCHITECTURE.md` is the implementation detail.

The engine implements (4) through an isolated group and a terminal shader draw
with the requested mode (engine mapping decision 21). Unsupported shader variants
return typed errors rather than an alpha-only substitute.

## Pinned by

- `flui-engine`: `layer_blend_tests::gpu_tests::a_layer_composites_its_whole_region_with_its_mode`,
  `damage_readback_tests::an_effect_layer_composites_its_whole_region_with_its_mode`,
  `damage_readback_tests::a_removed_translated_src_save_layer_leaves_nothing_behind`,
  `damage_readback_tests::a_removed_destination_affecting_layer_leaves_nothing_behind`,
  `damage_readback_tests::a_change_beside_a_viewport_compositing_layer_matches_a_full_frame`.
- `flui-painting`: `damage_extent::a_bounded_save_layer_extent_is_its_mapped_bounds`.

The engine-side detail is mapping decision 19 in `crates/flui-engine/ARCHITECTURE.md`.
