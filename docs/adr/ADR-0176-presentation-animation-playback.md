# ADR-0176: Animation playback belongs to a presentation clock

- **Status:** Accepted
- **Date:** 2026-10-08
- **Related:** [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md),
  [ADR-0095](ADR-0095-agent-protocol-schema-crate.md).

## Context

A process-wide animation multiplier couples independent windows and applies
speed changes to already elapsed time. A paused controller also needs a way to
produce one inspected frame without keeping the event loop active.

## Decision

Each presentation owns a monotonic `MotionClock`. It maps raw frame time to
typed animation ticks, applies rate changes at the current timeline boundary,
and admits explicit forward steps. A zero rate pauses ordinary progression.
Controllers apply their own playback rate to elapsed animation time, preserving
the current sample when that rate changes. There is no process-wide multiplier.

A controller's velocity is measured per input animation second at the last
accepted sample. A curved run reports the signed span times the curve derivative,
divided by run duration and multiplied by its applied controller rate. Paused
and stopped runs report zero. Derivative callbacks run outside state borrows;
replacement, stopping or a nested sample invalidates the old derivative read,
which reports zero. Source retirement retains the enclosing first failure.
Non-finite or unrepresentable derivatives report zero; regrouping the finite
factors preserves representable products at extreme rates and durations.

Position, raw elapsed time, local elapsed time and applied controller rate
commit together after the source returns a finite position and its sample
identity survives. A source panic or rejected position leaves the previous
sample authoritative. A failed completion query also leaves it intact. A pending
rate remains deliverable on the next successful sample, while a newer rate
requested during source evaluation remains pending for a subsequent sample.
Reserving a sample identity before source evaluation still invalidates an outer
sample when the same controller is ticked reentrantly.

Retargeting prepares replacement motion from the last published position and
velocity. Admission commits the new run before cancelling the displaced future.
Curve segments correct both endpoints with Hermite terms: inherited velocity at
the interruption and zero velocity at exact arrival. Replacement continues from
the published frame origin, so its first frame advances without a new hold.

An interruptible spring run completes at the exact target at rest, independent
of the frame that observes completion. Its native trajectory lasts until the
physical rest threshold, followed by a cubic Hermite transition preserving
the incoming position and velocity. The transition lasts at most one inverse
natural frequency; its inherited velocity displacement is capped by the
position tolerance. Stopping at the threshold would freeze a frame-dependent
near-target value and cut off residual velocity. Continuing the analytic tail
after completion would require frames for a run already reported as stopped.
The finite transition keeps completion, frame demand and published values
consistent. This applies to interruptible motion; standalone simulation rest
semantics remain those of the physics contract.

Typed `AnimatedValue` motion shares the controller's admission, sample identity
and delivery machinery. One owner registers all components; cloneable observers
retain the published value without prolonging motion. Generated components stage
outside controller borrows and commit one vector with the accepted clock. Identity
checks between position and velocity callouts stop displaced sampling. Exact
target representation survives settling even when its vector loses information.

Implicit opacity, padding and rotation consume that observed motion directly.
Programmatic scroll commands retarget their existing driver, synchronizing only
external position writes. A moving position asked to stop where it currently is
retains its incoming velocity and brakes; equality is an immediate fast path only
at rest. Scroll and page methods use `ArcCurve`, matching the motion contract.

The runtime's exact window agent port admits `MotionRequest` through the same
owner inbox and close fence as semantics operations. The owner validates the
whole request before changing rate or time. An invalid rate applies neither
field and answers the existing `invalid_argument` error. Accepted state is
replied before calling the platform wake hook.

An explicit step creates demand for its presentation. A paused registry alone
does not create ongoing animation demand. Testing's extra presentations expose
the same clock operations and keep their own rates and origins.

Run admission, playback-rate changes and registration migration request their
first sample through the live registry seat. The controller queues this demand
alongside its committed deliveries, so unchanged directional status does not
suppress a restart's wake. Registry addresses hold weak ownership and never
reuse slots; unregister revokes the route. Nested registries forward demand
through their live parent seats, subject to every ancestor's mute gate.
Unmuting requests a retained running sample.

The presentation binds its registry to its frame request capability. Callback
invocation and capture retirement occur outside controller and registry borrows
under the existing first-failure custody. Missing or failed delivery leaves the
accepted run installed; installing a replacement requests retained demand.
The scheduler's wake capability records platform delivery debt independently
of its frame latch. A run does not retain the UI runtime through that capability.
Closing a presentation revokes its driver authority even if a caller retains
the registry. Headless registry replacement revokes the outgoing authority
before invoking or retiring callbacks.

Protocol 0.2 adds `MotionRequest` and `MotionState` without changing existing
wire types. Devtools exposes the operation as `motion` with an exact window
handle. Missing request fields read the current clock. A mutating request that
times out remains queued, reports `may_have_run`, and must not be retried
automatically; callers can read its clock state first.

## Evidence and consequences

`agent_motion_sets_rate_and_steps_a_paused_window` pins owner admission,
atomic validation, accepted state and closed-window refusal.
`motion_op_round_trips_over_the_endpoint` exercises the actual local endpoint.
`presentation_rates_pause_and_step_are_independent` drives two testing
presentations through different rates, pause and one-frame step.

`starting_an_idle_bound_controller_requests_its_first_frame` and
`starting_an_idle_presentation_animation_requests_its_first_frame` assert the
request before any forced pump; removing run-admission demand fails both.
`driven_controller_owns_its_seat_and_run` covers same-status restarts, rate pause
and resume, nested mute, migration, competing wake/listener failures, hook
replacement and reentrant capture retirement.
`curved_run_velocity_is_the_curve_slope` checks the owning controller against
independent Bézier derivatives in both directions and at different rates.
The `retarget_seams` table covers linear endpoints, zero duration, pausing,
discontinuous curves, invalid slopes and extreme representable products.
`controller_sources_allow_reentry_and_preserve_run_ownership` covers derivative
reentry, stale-read refusal, competing failures and a subsequent successful run.
`controller_retarget_is_c0_and_c1_at_the_seam` covers rejected and panicking
positions, a failed completion query, retained pending rates and a subsequent
run. `controller_retarget_frame_boundaries` compares the next registered sample
after a rejected frame with an independently driven controller whose time starts
at the last published seam.
`closed_presentation_animation_cannot_wake_a_surviving_window` proves that a
saved clock cannot schedule a sibling after owner teardown.

`owning_animated_value_contract` covers atomic components, owner release during
sampling, exact-target delivery, reentrant conversion and retained deadlines.
Owning implicit property motion admits finite components only. Non-finite initial
optional properties are omitted; a non-finite update preserves the previous
numeric goals and their motion. This is admission to owning motion, independent
of ADR-0149's `Lerp` input domain. Container properties have independent owners
under the same presentation registry and notify one inner builder through weak
relays. Matrix targets keep ADR-0149 decomposition: replacement re-anchors the
displayed matrix with C⁰ continuity and runs separate curve or spring progress.

The mounted widget velocity rows exercise opacity, padding, alignment, container size and rotation through
their render, layout and transform producers. Painted container alpha continuity
is measured within its 8-bit quantization. The two mounted scroll replacement
rows assert pixel velocity continuity, exact settlement and activity completion.

The wire schema golden and additivity gate cover published protocol shapes.
Required fields on a newly introduced response type do not change older
requests or replies; required fields added to an existing type remain a
breaking change. Native platform timing and GPU output require their own
execution evidence beyond these headless checks.
