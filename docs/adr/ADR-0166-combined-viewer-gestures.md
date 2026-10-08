# ADR-0166: Combined viewer gestures and native source ownership

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0098](ADR-0098-owned-f64-geometry-values.md),
  [ADR-0159](ADR-0159-gesture-arena-owner-delivery.md),
  [ADR-0161](ADR-0161-immutable-owner-local-gesture-recognizers.md),
  [ADR-0162](ADR-0162-checked-pointer-plane-coordinates.md)

## Context

Independent competing Drag and Scale actors cannot preserve an established
one-finger pan when a second contact arrives: the first arena may already have
rejected Scale. Native pan-zoom has no Down admission, and transforming content
from every raw observer applies the same source to multiple nested viewers.

## Decision

`ScaleGestureRecognizer` keeps its two-contact `ScaleStartMode::Scale` default.
Explicit `PanOrScale` owns one combined sequence from its first recognized contact
until the final contact leaves. Adding or removing contacts rebases focal motion,
span and rotation without restarting the sequence or introducing a visual jump.
`InteractiveViewer` uses this mode through its persistent `GestureDetector` Scale
actor. Weak listener attachments do not extend the actor's lifetime.

Native input reaches that same actor through borrowed
`PanZoomDispatch { local, global }`. Local coordinates and the original root-space
source remain distinct. Native observers still receive fresh hit-tested input;
the detector's built-in recognizer attachment selects contact events only so raw
observation cannot mutate the native actor before arbitration.

Native consumption is leaf-first. A viewer refuses its first meaningful Update
unless its authored pan, scale or rotation configuration admits a finite transform
that can change the scene within its bounds. Start stages both coordinate-space
events; the first admitted Update recognizes the sequence. The binding retains
the exact claimant and its admitted transform through terminal delivery. Enabling
a descendant during a rebuild cannot steal the accepted source. After End or
Cancelled, the next source may choose that descendant.

Source matching uses PointerId and DeviceId; mutable tool or role metadata does
not define ownership. The binding groups a known device's native source under
that DeviceId and checks its pointer identity; without a device it uses PointerId.
Admission has an exact owner-local generation. Terminal withdrawal precedes
observers and callbacks. A repeated exact Start retires the actor's previous
generation while preserving the selected consumer for the replacement. Reentrant
replacement cannot be retired by old terminal work. Callbacks run outside borrows
and containment preserves the first failure.

Started native scale and rotation are cumulative. Independent Updates without
Start remain relative one-step interactions. Native localization follows
ADR-0162: at each current source focal, cumulative pixel pan is the checked chord
`U(focal + pan) - U(focal)`. It does not infer a starting global focal or accumulate
local steps. Scale and rotation remain dimensionless. Scroll line and page counts
remain source counts until the viewer resolves them using its line metric or
actual viewport dimension; they are never projected as pixel vectors.

Viewer transforms compose around the moving local focal point. Scale is clamped
to authored bounds; rotation is disabled by default and explicitly enabled.
Finite intermediate arithmetic, inverse geometry and the proposed matrix are
checked before publication. Containment uses the inverse viewport quad's bounds
against the axis-aligned scene boundary. If the quad cannot fit at the admitted
scale, the proposal is refused instead of silently enlarging scale. Boundary
correction may move the focal pivot when containment requires it. Subsequent
admissible proposals remain usable.

Scalar scale velocity is measured in scale units per second. Focal velocity is
separately measured in local logical pixels per second. Hardware event times and
measured history contribute to estimation; predictions do not. The viewer forwards
both quantities in its terminal details. Only focal velocity supplies translation
inertia; this does not introduce zoom or rotation inertia.

Focal inertia uses `FrictionSimulation` and an `AnimationController` registered
with the presentation's `VsyncScope` during lifecycle initialization. The friction
policy retains ten percent of release speed after one second; library tolerance
determines rest. Without Vsync there is no wall-clock substitute. Axis projection
and boundary containment apply to translation. A new Down, gesture recognition, accepted
wheel input, controller replacement and disposal stop the run. Per-run origin,
viewport and boundary are captured at release; an unchanged rebuild does not
restart the simulation. This contract does not promise that a later geometry-only
rebuild replaces those captured limits. A fresh liveness token prevents retired
listener work from publishing into a later run.

## Consequences and verification

One actor owns contact pan, pinch, rotation and admitted native pan-zoom. The
presentation binding owns native routing debt separately from fresh observation.
This avoids a second widget resolver and preserves ownership across rebuilds.

The public widget `pointer_and_gesture_recognition` table includes
`viewer_pan_transitions_to_pinch_without_contact_count_jumps`,
`viewer_native_pan_moves_the_scene_under_the_focal_point`,
`viewer_native_session_reports_one_start_and_one_terminal`,
`viewer_repeated_native_start_retires_the_previous_generation`,
`viewer_native_owner_survives_descendant_enable_during_rebuild`,
`viewer_native_rotation_preserves_the_scene_pivot`,
`viewer_rotation_refuses_an_unfittable_quad_then_recovers`,
`viewer_reports_scale_velocity_separately_from_focal_velocity`, and
`viewer_focal_fling_advances_then_stops_on_new_input`.
Existing Scale tables retain the default two-contact contract. These owned-event
witnesses do not claim physical trackpad or touchscreen execution on every backend.
