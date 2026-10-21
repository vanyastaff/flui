# ADR-0231: A superellipse clip hit-tests the curve it draws

- **Status:** Accepted
- **Date:** 2026-10-17

## Context

The engine drew `ClipSuperellipseLayer` with per-corner `|x/rx|⁴ + |y/ry|⁴ ≤ 1`
membership after scaling oversized radii to fit, but nothing produced the layer
from the render tree, and `RSuperellipse` had no point test: a dead helper used
exponent 2.5 without normalization. A clip that paints one shape and hit-tests
another sends taps to content the user cannot see, or drops taps on visible
content.

## Decision

`RSuperellipse::normalized` holds the one radius-fitting rule (a common factor
per shape, computed without summing radii so finite radii never overflow), and
`RSuperellipse::contains` evaluates the same exponent-4 corner curve on the
normalized shape with half-open edges. The engine's clip-chain packing calls
`normalized`, so the GPU mask and hit testing read the same radii.

`RenderClipRSuperellipse` (`RenderClip<RSuperellipse>`) produces the layer
through `PaintClip::RSuperellipse`, hit-tests with `contains`, and reports its
outer rectangle as conservative semantic bounds. `RoundedClipGeometry` gives the
border-radius setters to the rounded rectangle and the superellipse only. The
`ClipRSuperellipse` widget takes a `BorderRadius` like `ClipRRect`.

## Verification

`clip_layers_read_back_as_the_clip_contract_specifies` compares
`RSuperellipse::contains` with the GPU mask at every pixel clear of the
boundary, for the uniform icon squircle and for asymmetric radii larger than
the sides. `harness_clip_rsuperellipse_hit_tests_the_drawn_curve` pins a hit
inside the superellipse corner that a circular corner would reject, and
`a_superellipse_clip_composites_and_updates_its_radius` pins the widget's create
and update paths.
