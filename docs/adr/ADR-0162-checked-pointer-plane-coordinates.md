# ADR-0162: Checked pointer-plane coordinates and focal displacements

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md),
  [ADR-0113](ADR-0113-finite-inverse-hit-admission.md),
  [ADR-0143](ADR-0143-flui-owned-input-event-vocabulary.md)

## Context

A screen point supplies x and y without depth. Applying a full inverse paint
matrix to `(x, y, 0, 1)` invents screen depth zero: under perspective, this can
admit the wrong child and deliver coordinates different from the point that
painted the pixel. Subtracting inverse-transformed vectors at global origin
has a related defect: a projective displacement depends on its focal point.
Transforming scroll line or page counts as vectors also changes the quantity
before the receiving widget resolves it against its own metrics.

Rendering and interaction therefore share a checked local-plane operation.
Painting retains its separate forward projection semantics.

## Decision

### Positions and admission

`Matrix4::unproject_to_plane(x, y)` takes an **inverse global-to-local matrix**
obtained through checked inversion of the forward paint transform. It intersects
the screen ray with the receiving homogeneous `z = 0` plane before dividing by
its weight. A parallel ray, intersection at infinity, non-finite result,
cancellation within floating-point precision, or non-positive forward
homogeneous weight refuses traversal or that target's localized event.
The operation does not invent a finite point at a horizon or behind the camera.

Finite plane-aligned affine arithmetic uses its original multiply/add/divide
order. Normalizing ordinary translation and scale first would introduce
roundoff into previously exact delivered coordinates. When direct arithmetic
cannot produce finite coordinates, and for projected planes, positive
normalization bounds homogeneous products while preserving quotient and
visibility sign. The depth-elimination pair is normalized independently;
plane-aligned evaluation excludes irrelevant depth coefficients. An admitted
anisotropic inverse must not lose its meaningful 2D quotient merely because
its depth scale is very different.

Computed inverse admission retains ADR-0113's range limits. This operation
does not promise recovery of a transform whose determinant or inverse was
already refused. `Matrix4::transform_point` and rectangle painting keep their
existing forward projection contract.

`RenderTransform`, `RenderContainer`, `RenderFlow`, `RenderAnimatedTransform`,
`PipelineOwner::global_to_local`, and interaction localization use the shared
primitive. Hit entries retain full inverse matrices. Current, coalesced and
predicted positions use the same receiving-plane check; one refused sample
refuses the localized event for that target. Times, pointer/device identities,
buttons, modifiers and sensor readings remain unchanged.

### Displacements and source provenance

Pixel scroll displacement is the checked chord
`U(focal + delta) - U(focal)`, where `U` is receiving-plane unprojection.
Both endpoints, source endpoint addition, and final subtraction must remain
admissible and finite. An invalid endpoint refuses localized delivery.

Scroll `Lines` and `Pages` retain their exact source counts and unit while the
focal point is localized. The consuming scrollable converts these counts using
its line metric or actual viewport dimension. They are not hypothetical pixel
endpoints; a count is preserved even when treating it as a pixel displacement
would cross a horizon.

Native pan is cumulative since Start, while an Update's position is its current
focal. Its local cumulative pan uses
`U(current_focal + cumulative_pan) - U(current_focal)` independently for every
Update. Routing does not accumulate local steps or infer a starting global
focal. Scale and rotation remain dimensionless and unchanged.

`PointerDispatch` and native `PanZoomDispatch` borrow both `local` and `global`:
the former carries receiving-plane geometry, and the latter preserves the
complete original source. `Listener` and `GestureDetector` pass the native pair
to the scale recognizer; `InteractiveViewer` consumes the resulting gesture
details. Consumers that need source cumulative values use the global event
rather than reconstructing it from localized geometry.

## Behavioral witnesses

The public `hit_test_matrix` table exercises the actual transform, container,
flow and pipeline consumers. A Y rotation with cos=0.6 and sin=0.8 followed by
`w = 1 + z/10` projects local `(2,3)` to screen `(10/7,25/7)`. The old
depth-zero inverse gives local x=6/7, wrongly admitting a 1.5-wide child.
The table also covers hidden, horizon, near-horizon and edge-on admission.
`hit_test_transform_admission` checks actual owner-lane delivery of measured
and predicted sample families, source metadata, refusal, and tiny affine
coordinates. Anisotropic controls vary x scale `1e-200` and z scale `1`/`1e200`
after asserting computed inverse admission.

`transformed_entry_receives_local_samples_and_deltas` compares ordinary pointer
delivery and scroll claims. For forward projection `(x,y)/(1-x/2)`, source pan
`(1,0)` gives local chord `(2/3,-2/3)` at screen focal `(0,2)` and
`(1/5,-1/5)` at `(2,2)`. Its refusal rows cover horizon, behind-plane,
near-horizon and finite-input endpoint overflow.
`page_scroll_resolves_against_the_actual_viewport` mounts transformed real
Scrollable widgets: half a line remains 26.5 logical pixels, and half a page
remains half the actual viewport dimension.

Restoring finite depth-zero inverse projection fails the position and admission
witnesses. Restoring origin-based vector mapping and geometric line conversion
fails the focal/refusal table and the real transformed Scrollable witness.
Source-owner lifecycle is separately pinned by
`viewer_native_session_reports_one_start_and_one_terminal` and
`viewer_native_owner_survives_descendant_enable_during_rebuild`; these lifecycle
rows do not substitute for the geometric witnesses.

## Consequences

Geometry owns the coordinate convention shared by rendering and interaction.
Callers receive a deliberate refusal rather than non-finite or fabricated
local geometry, while ordinary affine coordinates retain their arithmetic
behavior. General 3D scene intersection and camera near/far clipping remain
outside the receiving-plane contract.
