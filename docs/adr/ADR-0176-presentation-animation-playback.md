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

The wire schema golden and additivity gate cover published protocol shapes.
Required fields on a newly introduced response type do not change older
requests or replies; required fields added to an existing type remain a
breaking change. Native platform timing and GPU output require their own
execution evidence beyond these headless checks.
