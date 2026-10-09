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
`closed_presentation_animation_cannot_wake_a_surviving_window` proves that a
saved clock cannot schedule a sibling after owner teardown.

The wire schema golden and additivity gate cover published protocol shapes.
Required fields on a newly introduced response type do not change older
requests or replies; required fields added to an existing type remain a
breaking change. Native platform timing and GPU output require their own
execution evidence beyond these headless checks.
