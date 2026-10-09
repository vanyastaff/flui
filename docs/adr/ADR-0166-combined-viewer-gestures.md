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
`PanZoomDispatch::new(local, global)`. Local coordinates and the original root-space
source remain distinct. `at_root` constructs the coincident-space form. The
dispatch is owner-affine: it implements neither `Send` nor `Sync` and does not
implement `UnwindSafe` or `RefUnwindSafe`. Its private
admission authority prevents external struct literals from fabricating a staged
binding admission. Synthetic constructors carry no admission authority.
Native observers still receive fresh hit-tested input;
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

Terminal admission is captured before fresh hit testing and raw observation.
Both can reenter the same Scale actor with a new Start. The outer terminal and
its retirement then carry only the original exact admission authority, whether
the original actor was dormant or recognized. Reusing PointerId, DeviceId and
EventTime does not make the new actor generation equal to the old one. End,
Cancelled and callback failure leave that replacement usable for its own Update
and terminal.

Binding-owned Start dispatch supplies borrowed admission authority. Each staged
actor registers retirement for its exact generation; the selected Update commits
the delivery owner before recognized callbacks. Losing actors retire even when
the current hit path no longer contains them. Withdrawal, replacement and closure
retire outstanding admissions outside borrows. A stale retirement cannot clear a
reentrant replacement that reuses its source and timestamp.
Failure while retiring an older staged generation does not reject its already
accepted replacement Start. The replacement remains deliverable before the
earliest failure resumes. Likewise, a terminal's fresh hit-test failure cannot
erase terminal delivery and retirement owed to its cached exact owner. Fresh
observation and admitted delivery have separate obligations; old cleanup cannot
withdraw a newer reentrant admission.
Native admission tickets use the binding owner's close-mode failure fence.
Healthy close invokes and retires each ticket outside borrows. After the first
failure, or during preserving close, opaque ticket callbacks and last-owner
captures are retained rather than starting another user callback or destructor.
This keeps native cleanup in the same ownership policy as the binding's other
accepted work; a separate inner containment accumulator cannot weaken that policy.
Pointer-sequence cancellation also carries its enclosing first-failure state into
native retirement. Accepted native cleanup callbacks remain deliverable after
an earlier CaptureLost failure; their opaque captures retain the earlier failure's
ownership fence under ADR-0127. Fresh admission after containment retires its own
captures normally and does not redeliver the cancelled generation.
`nested_native_scale_loser_recovers_touch_after_winner_terminal` checks winner
continuity, both terminal reasons and the losing ancestor's next touch gesture.

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
restart the simulation. A weak owner-local post-frame callback checks those
limits against completed layout. Changed viewport geometry or authored boundary
retires the old run before its next motion tick; a later release captures the new
limits. Translation still publishes on the Vsync listener before build and paint,
so geometry validation does not defer current-frame motion until post-frame.
Repeated samples with no elapsed time publish no motion and do not stop a live
run. Natural simulation rest removes its listener and stops Vsync requests. A
fresh liveness token prevents retired listener work from publishing into a later run.

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
`viewer_focal_fling_advances_then_stops_on_new_input`, and
`viewer_focal_fling_rebuild_preserves_or_retires_geometry`.
The interaction `binding_input_contract_matrix` includes
`native_staged_retirement_preserves_delivery_and_failure`,
`native_terminal_observation_failure_keeps_owned_delivery`, and
`native_staged_generation_survives_geometry_and_reentry`; these cover isolated
and competing failure, exact same-source replacement and healthy recovery.
`native_close_preserves_retirement_ownership` covers healthy and preserving close,
including first-failure capture retention.
`native_same_actor_terminal_reentry_preserves_new_generation` covers End and
Cancelled, dormant and recognized actors, fresh-probe and raw-observer reentry,
failure and subsequent same-source recovery.
`native_cancellation_retains_captures_after_prior_capture_failure` specifies
required native cleanup, safe capture retention and fresh same-source recovery
through both public pointer-sequence cancellation methods.
Existing Scale tables retain the default two-contact contract. These owned-event
witnesses do not claim physical trackpad or touchscreen execution on every backend.
