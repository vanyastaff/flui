# ADR-0149: Interpolation contracts

- **Status:** Accepted (2026-10-06). The owner chose Oklab with premultiplied alpha as the colour
  default and the collapsed-axis rule for matrices on 2026-10-06.
- **Date:** 2026-10-06
- **Supersedes:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md) §7, in part: the bullet
  saying `Color::lerp` interpolates premultiplied components (in gamma-encoded sRGB). The rest
  of §7 stands, including the gradient defaults (Oklab, premultiplied), which `Color::lerp` now
  shares.
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md) §2 (one `f64` scalar, narrowed
  once)

## Context

`Lerp` (`flui-foundation`) is the one interpolation trait behind `Tween<V>` (`flui-animation`),
and `Color` (`flui-painting`) and `Matrix4` implement it. Three crates therefore share one
contract that was written down nowhere, and it had drifted:

- `Matrix4::lerp` decomposed with glam's scale/rotation/translation split. A zero scale axis
  divides by zero inside it, so every interior frame of a scale-in from zero was NaN, and skew
  and perspective were silently dropped.
- `Color::lerp` mixed gamma-encoded sRGB (premultiplied), while a second public path,
  `Color::lerp_oklab`, mixed Oklab with straight alpha, so a fade to transparent darkened on it.
- `Tween` and `Lerp` extrapolate past `[0, 1]` for overshooting curves, but nothing said who
  keeps an extrapolated padding or size inside its domain, and `AnimatedContainer` handed
  negative insets to layout.
- An angle had no type: a `Matrix4` rotation always took the shorter arc, and a multi-turn
  rotation could not be written as a target.

## Decision

1. **`Lerp` extrapolates; the property clamps.** `t` outside `[0, 1]` extrapolates. A value's
   domain (a non-negative padding, size or radius) is enforced by the consumer that applies it
   to the property, at the point of use, not by `Lerp` and not by `Tween`. Types with a NaN
   representation (the `f64` geometry, `Angle`) carry NaN through; `Color` and the integer
   tweens return their beginning value for a NaN `t`.
2. **Colour interpolates in Oklab with premultiplied alpha** (CSS Color 4, "Interpolating with
   Alpha"). Both endpoints become premultiplied Oklab vectors (`L`, `a`, `b` times alpha), mix,
   and are divided by the mixed alpha; with no alpha left the components mix straight.
   Out-of-gamut results clamp per channel and alpha saturates. `t = 0` and `t = 1` return the
   endpoints exactly. `Color::lerp` (which clamps `t`) and `Lerp for Color` (which extrapolates)
   share this one path; there is no other public colour interpolation. The conversion pair
   `Color::to_premultiplied_oklab` / `Color::from_premultiplied_oklab` is public so a
   component-wise animator (a spring over a colour) runs in the same space.
3. **`Matrix4::lerp` decomposes like CSS Transforms 2** ("Decomposing a 3D matrix") into
   perspective, translation, scale, skew and a rotation quaternion, interpolates each linearly
   and the quaternion by spherical interpolation along the shorter arc, and recomposes. An
   endpoint whose linear part collapses an axis (zero scale) has no orientation of its own and
   takes the other endpoint's rotation and skew; this rule is FLUI's own (CSS has no such
   case). A matrix that cannot be decomposed (`m33 = 0`, an `m33` so small that normalising overflows, or a perspective row over a singular
   linear part) switches discretely at `t = 0.5`. Endpoints are returned exactly, and finite
   endpoints give finite output.
4. **Angles are `Angle`** (radians in `f64`, whole turns kept). Interpolation is numeric, so a
   multi-turn rotation is expressible (CSS Transforms 1, "Interpolation of Transforms"); the
   shorter arc is the consumer's choice, through `Angle::nearest_equivalent`, whose exact
   half-turn tie goes to the increasing angle.

## Alternatives considered

- **Clamp inside `Lerp` or `Tween`.** Rejected: it flattens overshoot for values whose domain is
  unbounded (an `Offset`, an `Alignment`), and the trait cannot know which is which.
- **Keep sRGB as the default and fix `lerp_oklab`'s alpha.** Rejected by the owner: two public
  paths with different results, and the default the less perceptual one. Gradients already
  default to Oklab (ADR-0098 §7).
- **Only guard the zero axis in the glam split.** Rejected: skew and perspective would still be
  dropped.
- **Delete `Lerp for Matrix4`.** Rejected: `AnimatedContainer::transform` consumes it.
- **Element-wise or normalised-linear matrix interpolation.** Rejected: element-wise shears a
  rotation, and neither keeps skew or a rotation's angular speed.

## Consequences

- Every `Tween<Color>` frame between the endpoints changes: a black-to-white midpoint is
  `rgb(99, 99, 99)` instead of `rgb(128, 128, 128)`, and red half way to black is
  `rgb(99, 0, 0)`. Endpoints are unchanged.
- A colour lerp costs two Oklab conversions (`powf`, `cbrt` per channel), about 175 ns instead
  of about 11 ns on the reference host.
- `Color::lerp_oklab` and `OklabColorTween` are removed.
- `Matrix4::lerp` keeps skew and perspective and no longer publishes NaN for a zero scale axis.
- `AnimatedContainer` clamps padding, margin, width and height into their domains, and animates
  `transform`; `AnimatedRotation` is the first `Angle` consumer; hero flights keep a
  non-negative shuttle size.

## Verification

- `flui-foundation`: `matrix4_lerp_decomposes_like_css_transforms`,
  `matrix4_lerp_endpoints_and_finiteness` (property test),
  `angle_nearest_equivalent_takes_the_shorter_arc`.
- `flui-painting`: `color_lerp_is_premultiplied_oklab`.
- `flui-animation`: `integer_tweens_interpolate_across_the_full_range`.
- `flui-widgets` contract rows: `animated_rotation_takes_the_shorter_arc`,
  `animated_rotation_takes_the_numeric_arc`, `animated_container_animates_its_transform`,
  `overshooting_padding_stays_non_negative`, `overshooting_margin_stays_non_negative`,
  `overshooting_size_stays_non_negative` and
  `a_shrinking_flight_with_overshoot_keeps_a_non_negative_size`.
