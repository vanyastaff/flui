# ADR-0155: Ballistic rest tolerance derives from the presentation's device pixel ratio

- **Status:** Accepted
- **Date:** 2026-10-06
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md)

## Context

A scroll fling, page snap or overscroll spring-back is a `flui-animation`
simulation that decides on its own when it is done. Its rest tolerance used to
be a fixed constant in logical pixels, so the same fling stopped visibly short
on a high-density display and ran on invisibly on a low-density one. The
tolerance that matters is what the user can see: a fraction of a device pixel.
The density belongs to the presentation (`flui-rendering`'s `PipelineOwner`),
the tolerance type to `flui-animation`, and the consumer to `flui-widgets`.

## Decision

1. `flui-animation` owns the conversion: `Tolerance::for_device_pixel_ratio(r)`
   is a distance of half a device pixel (`0.5 / r` logical pixels) with no
   velocity limit, and refuses a ratio that is not finite and positive.
2. `flui-widgets` reads the ratio of the presentation the scrollable belongs to
   at release time (not at build time, so a window moved to another monitor is
   honoured), carries it in `ScrollMetrics::device_pixel_ratio`, and every
   ballistic simulation its physics build (the built-in `ScrollPhysics` and
   `PageScrollPhysics`, which the refresh indicator also drives) rests within
   `ScrollMetrics::ballistic_tolerance()`.
3. When the presentation cannot be read (it is gone, or a frame holds its
   pipeline) the ratio is `1.0`. A ratio the conversion refuses produces no
   ballistic simulation rather than one with a made-up tolerance.

## Consequences

- A fling rests at the same visible distance on every display; its duration
  grows with density (pinned by `scroll_fling_rest_scales_with_device_pixel_ratio`).
- Third-party physics receive the ratio in `ScrollMetrics` and should build
  their tolerance through the same conversion.
- No other presentation value reaches the simulations: a velocity limit or a
  frame-rate-dependent rest would need a new decision.
