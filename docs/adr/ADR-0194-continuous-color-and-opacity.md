# ADR-0194: Color and group opacity retain normalized precision until rasterization

- **Status:** Accepted
- **Date:** 2026-10-11
- **Supersedes:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md), the implementation-status deferral of normalized `Color` storage only.
- **Related:** [ADR-0149](ADR-0149-interpolation-contracts.md), [ADR-0190](ADR-0190-owner-local-render-invalidation.md).

## Context

An animated color can have distinct alpha samples that round to the same byte.
Quantizing before paint modulation loses information: an image filtered to
alpha 0.0025 or 0.005 and then modulated by 0.5 should reach the renderer as
different source values. Baking the filter into an RGBA8 image made both
samples identical. Byte-quantized group opacity similarly suppressed accepted
animation updates, including the transition between hidden and visible semantics.

These failures concern ordinary sRGB rendering. They do not require HDR or a
wide-gamut public color model.

## Decision

`Color` owns private, finite `f32` straight-alpha sRGB components in `[0, 1]`.
Construction canonicalizes negative zero. Saturating construction maps NaN and
negative values to zero and values above one to one. Transparent colors retain
their authored hidden RGB. Exact equality and hashing compare the admitted
components; colors with equal byte exports can still be distinct colors.

Byte authoring remains explicit: `rgb`, `rgba`, `from_argb`, hexadecimal parsing
and byte channel replacement keep their existing units. `to_rgba8`, `to_argb`
and hexadecimal formatting explicitly round at the byte boundary. Normalized
interop uses `from_rgba_f32_array` and `to_rgba_f32_array`; channel accessors name
their `f32` units. No public field permits bypassing admission. The `color` crate
provides internal color-space conversion; its types do not enter FLUI signatures.
Oklab interpolation and component animation retain ADR-0149's premultiplied contract.
Component-motion admission preserves the exact displayed origin until time
advances, including transparent colors' hidden RGB. Completion returns the exact
authored target. A running interruption preserves its displayed seam; component
conversion may be lossy and is not used to reconstruct exact endpoints.
Observers retain both endpoints of the current segment until replacement or
observer retirement. Endpoint destruction occurs outside sample commits, through
the existing animation retirement boundary.
Extreme extrapolation scales the affine Oklab mix before evaluating it, then
restores linear-light magnitude using the cubic homogeneity of Oklab conversion.
It saturates output channels rather than truncating the interpolation parameter.
Infinite parameters use the corresponding directional limit. Conversion remains
subject to floating-point precision, including subnormal endpoint underflow.

Optional serde uses `{ "srgb": [r, g, b], "alpha": a }` in normalized units.
Deserialization rejects unknown fields, non-finite values and values outside
`[0, 1]` before narrowing to storage precision. This intentionally replaces the
former byte-channel object; persisted data must migrate each byte to `byte / 255`.

`Color::luminance` reports relative linear sRGB luminance, excluding alpha.
Dark/light classification and text selection compare black and white using the
[WCAG contrast ratio](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html).
The greater ratio wins; ties choose black. A translucent surface is composited
over its actual opaque backdrop before selection. This replaces gamma luma and
the fixed 0.5 threshold, which selected insufficient-contrast white text on
ordinary opaque gray and red backgrounds. Color selection alone does not prove
application accessibility. These four methods become non-const because stable
sRGB decoding uses non-const floating-point exponentiation. Packed byte exports
retain constant evaluation.

`PaintOpacity` owns admitted normalized `f64` group alpha. Render objects expose
their accepted opacity rather than a byte approximation or a fresh user-code
read. Exact zero suppresses paint; exact one permits the opaque path. Interior
values remain available to retained layer updates. Animated opacity invalidates
semantics when accepted visibility crosses zero. `always_include_semantics`
retains the child even at zero opacity. These are independent of hit testing.

Decoded image filters operate on each straight source texel before
premultiplication, interpolation and paint modulation. Cached textures retain
the original image; a filter descriptor participates in batch compatibility.
Mode, Matrix and Gamma image filters share arithmetic with their layer-filter
counterparts. Invalid matrix coefficients omit the image operation. Finite CPU
matrix evaluation uses widened intermediates; GPU evaluation bounds arithmetic
before multiplication to avoid relying on overflow behavior. Neither promises
exact arbitrary cancellation residuals beyond its floating-point precision.

Filter uniforms and bindings participate in preparation budgets. Unsupported
device limits return a typed resource-limit error before creating the filtered
pipeline. A refused replay clears its pending cached-image batch; after the
caller discards the failed encoder, a subsequent frame does not replay that
rejected batch. Ordinary images retain their existing device requirements.

## Alternatives and consequences

Keeping byte storage and changing only the tween leaves modulation, filters,
derived component animation and group opacity subject to the same loss.
Providing a parallel high-precision color type requires consumers to know which
producer loses information. Normalized storage gives the existing color type
one contract. Float texture baking adds a second image cache format and still
requires a deliberate filtering stage; source sampling expresses that stage
directly without replacing the original texture.

This is an intentional pre-1.0 Rust and serde compatibility change. Replace
color field reads with normalized accessors or an explicit byte export; replace
color struct literals with constructors. Replace render-opacity byte access
with accepted normalized opacity. Full byte round trips no longer describe all
colors. Native RGBA8 attachments and decoded source bytes remain quantized;
this decision does not promise lossless pixels or CSS Color conformance.

Continuous Oklab conversion can cost more than the former byte lookup path.
Operation benchmarks and active animation measurements must distinguish that
cost from whole-frame performance; correctness does not establish a performance
budget. Wide-gamut, HDR and alternative gradient spaces remain separate decisions.

## Verification

Public painting rows `normalized_admission_and_hashing_are_canonical`,
`fractional_channels_survive_color_operations` and
`color_wire_validates_normalized_units_before_narrowing` pin admission, precision
and wire units. Animation's `color_motion_publishes_fractional_running_samples`
and widgets' `container_color_retarget_preserves_painted_alpha_progress` pin
actual component motion and recorded paint.
`admitting_motion_preserves_the_exact_displayed_origin` pins exact admission,
zero-time sampling, interruption and explicit grouped restart through the public
owning value and its observer.

`contrast_selection_uses_linear_relative_luminance` compares every authored
gray channel, primaries, fractional RGB and composited translucent surfaces
against an independent f64 relative-luminance and contrast reference.

`harness_animated_opacity_semantics_follow_accepted_visibility` exercises the
committed semantics tree through box and sliver consumers. The public GPU
`composited_layer_update_readbacks` family covers fractional group and color
alpha, image-filter modulation, sampling order and batch identity.

Engine's `painter_images_and_offscreen_results_read_back_as_specified` includes
filter preparation refusal and recovery, independent antialiased coverage,
finite matrix limits and reduced device limits. The engine architecture mapping
decision documents their private resource seams and encoder recovery contract.
These scenarios establish the stated contracts; they are not a claim of full
WCAG, Web Animations or CSS conformance.
