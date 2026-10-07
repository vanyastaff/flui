### Fixed

- Gradient geometry preserves animation overshoot, including paired gradients in box decorations; colors remain saturated, radii stay within `0..=f32::MAX`, and interpolation rejects non-finite geometry.
