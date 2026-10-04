# ADR-0115: Images carry source regions, affine placement and Paint through replay

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0098 (logical and device geometry)

## Context

Decoration Cover and RenderImage Cover advertised cropping but recorded the
entire image into an overflowing destination. Image replay mapped only two
opposite corners, losing rotation and shear. Atlas replay discarded each
sprite's linear transform. Optional image Paint lost its color and alpha;
DecorationImage color_filter was not recorded. Ordinary cached images also
replayed with SrcOver regardless of their requested blend mode.

## Decision

A source-region image command carries an in-bounds texel rectangle, a logical
destination, optional fitted logical tile placement, repeat mode, filter and
optional Paint. Canvas conveniences and Decoration/RenderImage producers use
that command. BoxFit::apply determines source and destination extents;
alignment places both the cropped source and fitted destination. RenderImage
converts its intrinsic/scale logical source back into decoded texels.

Repeat coverage is separate from the fitted tile rectangle. The latter fixes
both logical tile extent and aligned phase. The first and last tiles crop UVs
rather than stretching the remaining image. Existing Canvas repeat keeps its
natural image pixel extent and destination-origin phase.

Engine recording accepts finite affine transforms and records the device
origin and both basis vectors. The GPU consumes that same quad; replay bounds
come from its four narrowed corners. Invalid or non-advancing geometry omits
the complete image operation. Recording quota refusal stays sticky and exits
immediately, using the existing shared budget. Repeat, slices and sprites
publish their admitted private segment only after the operation is complete.

ColorFilter processes decoded straight channels before source cropping,
filtering, Paint color/alpha multiplication and premultiplication. Bilinear
straight texels are premultiplied before interpolation. Source crops retain
normal image filtering at their boundary; they do not add an artificial
half-texel UV inset or redefine sampling as nearest. Each instance also carries
the original image's full UV bounds. Manual linear taps clamp to that image's
texels, preventing atlas gutters or neighboring entries from changing an edge
sample compared with a standalone texture. These bounds are not the crop bounds:
a fractional internal crop still samples its neighboring original-image texels.
Actual transparent texels retain their alpha. Gutter allocation itself is unchanged.

Ordinary cached draws retain their blend mode. Advanced modes isolate the
original image operation as one group. Destination-sensitive modes under
fractional clipping use the existing portable coverage compositor: sampled
premultiplied image color and geometric clip coverage occupy separate planes.
Transparent source pixels still replace the covered destination where the
operator requires it, while uncovered destination survives. Image pipelines
use this portable path on both dual-source and fallback devices; no additional
image backend is introduced.

## Verification

The existing `painter_images_and_offscreen_results_read_back_as_specified`
family covers rotated source orientation, shear membership, tint/alpha,
transparent Src writes and feathered Src destination preservation. Producer
fit/crop, repeat phase and filter rows join the same family. Counterfactual
mutations independently remove source crop, affine bases, Paint propagation,
blend routing and independent coverage. Existing repeat progress/quota rows
remain controls. Required GPU checks execute these readbacks without adapter
skips; passing results are recorded by the task's validation, not assumed here.
