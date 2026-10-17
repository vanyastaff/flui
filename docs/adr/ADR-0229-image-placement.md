# ADR-0229: Image placement is one painting contract

- **Status:** Accepted
- **Date:** 2026-10-17
- **Related:** ADR-0115 (image source regions), ADR-0098 (logical and device geometry)

## Context

`RenderImage` carried its own fit and alignment enums: five of the seven
`BoxFit` modes, nine fixed alignments, no repetition and no centre slice, while
`DecorationImage` painted every `BoxFit`, fractional alignments and repetition
through a separate copy of the same fitting code. The engine's nine-slice
command draws corners one logical pixel per texel, so it cannot honour an
image's scale.

## Decision

`flui_painting::ImagePlacement` places an image into a rectangle, and both
producers paint through it:

- `Fitted { fit, alignment, repeat }` applies any `BoxFit` to the image's
  natural (logical) size, positions the result at a fractional `Alignment` and
  optionally repeats it. The cropped source converts back to decoded texels
  axis by axis, so a scaled or non-uniformly sized image keeps its logical
  geometry. A natural extent past `f64::MAX` (a tiny scale) is bounded with
  its ratio preserved before fitting.
- `Sliced { center }` is a nine-slice: `center` is in decoded texels, the
  corners keep their natural size, edges stretch along their side, the centre
  in both directions, and the slice fills its rectangle. Corners wider than the
  rectangle shrink together on that axis. It records nine source regions
  rather than the engine nine-slice command, so scale is honoured on every
  renderer.

The variants are separate because they do not combine: a fit that crops or
letterboxes a slice, or a repeated slice, has no defined meaning, so the type
cannot express it. A centre outside the image, inverted or non-finite paints
nothing.

`RenderImage` stores one `ImagePlacement` (paint-only; layout never reads it).
The `Image` widget's `fit`, `alignment` and `repeat` builders edit a fitted
placement and `center_slice` replaces it with a slice; the last call wins.
`DecorationImage` keeps its fields and paints as a fitted placement at one
logical pixel per texel.

## Verification

`image_placement_contract` in `flui-painting` pins slice geometry at a 2x
scale, proportional corner shrinking, invalid slices, `FitWidth`/`FitHeight`
crops, fractional alignment and logical-size repetition.
`harness_image_placement_slices_and_repeats_in_logical_units` pins the mounted
render object; `image_placement_builders_reach_the_painted_scene` pins the
widget's create and update paths. The engine row
`sliced_placement_keeps_logical_corners` reads back pixels that differ for
texel-sized corners and for a whole-image stretch.
