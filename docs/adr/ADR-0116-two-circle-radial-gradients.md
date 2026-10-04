# ADR-0116: Two-circle radial gradient evaluation

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0098 (owned f64 geometry values)

## Context

The painting vocabulary already carries a radial focal point, focal radius and
four tile modes. Decoration resolved only the final circle; ordinary and advanced
GPU draws also evaluated only distance from its centre. Those paths accepted
configurations whose rendered result discarded the configured first circle.

## Decision

A radial gradient interpolates from circle `(F, r0)` at parameter zero to circle
`(C, R)` at parameter one. An absent focal point means `F = C`; an absent focal
radius means `r0 = 0`. Decoration resolves both alignment-relative centres and
both shortest-side-relative radii into the same local coordinate system.

For a point `P`, set `d = C - F`, `dr = R - r0` and `q = P - F`. The parameter
satisfies `A*t*t + B*t + D = 0`, where `A = dot(d,d) - dr*dr`,
`B = -2*(dot(q,d) + r0*dr)` and `D = dot(q,q) - r0*r0`. Select the largest finite
real root whose interpolated radius is finite and nonnegative. A point without
such a root contributes zero coverage. The exactly linear and repeated-root
cases are separate branches; there is no epsilon that changes the equation.
The quadratic branch uses the stable-root/product formulation to avoid
subtracting nearly equal terms in both roots.

An exactly linear equation with a zero coefficient for its parameter contributes
zero coverage, including a shared tangent point contained in every interpolated
circle: it has no unique parameter. Coincident zero-radius constant mode below
is the explicit exception.

Clamp extends endpoint colours. Repeat reduces the parameter modulo one;
Mirror folds it modulo two. Decal contributes zero coverage outside `[0,1]`.
These operations follow root selection. Rounded-box and inherited clip
coverage remain independent and evaluate derivatives before per-fragment
root branches.

Nonfinite centres/radii, negative radii and unsupported numeric packing produce
typed geometry errors before stop storage or advanced destination-read work.
Coincident equal nonzero circles are refused because their parameter is
undefined. Coincident zero-radius circles explicitly retain the existing
first-colour constant mode.

Coordinates are rebased in f64 and normalized by a shared finite scale before
packing. Admission rejects collapsed nonzero coordinates, circle differences,
quadratic/linear classification and rectangle-local subtraction. Packed inputs
bound the polynomial intermediates. This is a contract for computed GPU
arithmetic; it does not promise that every mathematically meaningful conic is
representable. A finite real root or its radius can still exceed f32 range;
that fragment has no admissible computed solution.

Ordinary and advanced draws share admission and the radial shader. Shader masks
remain narrower: Clamp gradients without focal parameters. Supporting focal
radial paint does not silently expand mask shader admission.

## Validation

Rows in `layer_effects_capture_as_specified` use public Canvas, decoration and
headless capture APIs. Expected colours come from independently chosen circle
boundary points, including initial-circle radius, a linear equation, repeated
root, both-root selection and a point outside the cone. SrcOver and Multiply
exercise the two recording paths. Default/zero-radius controls, tile modes and
typed refusal followed by a healthy frame preserve recovery and existing modes.
Verification results are recorded by the implementing task; this ADR alone
claims no executed GPU result.

## Reference check

After selecting FLUI's contract, Skia's
[conical gradient implementation](https://github.com/google/skia/blob/d2b9e48baf1697760afc1dc8ea3ad40110b8cacc/src/shaders/gradients/SkConicalGradient.cpp)
was checked. It distinguishes focal, concentric and equal-radius cases, masks
points outside the cone and defines its own coincident-circle policy. That
supports explicit case handling rather than treating a focal gradient as
ordinary distance from a different centre. FLUI retains its own refusal,
value and frame-admission contracts.
