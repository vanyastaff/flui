### Fixed

- Gradient geometry preserves animation overshoot, including paired gradients in box decorations; colors remain saturated, radii stay nonnegative with representable normalized values, and interpolation rejects non-finite or distorted geometry.
- Box decorations retain their endpoint color/stop ramp and terminal bounded gradient through chained extrapolation, selecting the fallback against the actual silhouette bounds when needed. Linear projection checks wait for the paint-box size so representable overshoot in small boxes remains supported.
