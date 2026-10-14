# ADR-0220: Animation misuse diagnostics

- **Status:** Accepted
- **Date:** 2026-10-13
- **Related:** [ADR-0198](ADR-0198-typed-animation-admission-errors.md), [ADR-0202](ADR-0202-animation-observation-through-tracing.md)

## Context

ADR-0198 made every animation refusal a typed `AnimationError` returned before
admission. Widgets and other built-in drivers often discard that result: a
looping indicator calls `repeat` and ignores the future, a gesture handler
calls `fling` and moves on. A refusal there is a silent no-op, and the first
sign of a misused controller is an animation that never moves. Only some
refusals were logged, as free-form messages on the crate's default target,
and a command on a disposed controller was not logged at all.

ADR-0202 puts optional observation (transition receipts, spans) behind the
`diagnostics` feature. Misuse is not observation: an application adopting the
framework needs it with the default features.

## Decision

Every public animation command that returns an `AnimationError` reports the
refusal once, at `warn`, on the `tracing` target `flui_animation::refused`
(exported as `REFUSAL_TARGET`). The event carries three fields:

- `operation`: the command, qualified by its type
  (`AnimationController::repeat`, `AnimatedValue::retarget`, `MotionUpdate::run`);
- `kind`: `AnimationError::kind()`, a stable snake-case name per variant;
- `error`: the error's `Display`;
- `label` and `created_at`: the refusing controller's debug label
  (`AnimationControllerBuilder::debug_label`) and the site that built it,
  when it has them. They are display fields: correlation stays on the
  controller span, which also carries them and, with the `diagnostics`
  feature, parents the event. A creation site is recorded only with debug
  assertions or the `diagnostics` feature and is zero-sized otherwise.

The report is emitted after the command has committed or refused and has
released every controller borrow, so a subscriber, which is user code, can
reenter the controller. A grouped `MotionUpdate` reports its one final
refusal, not each staged member's.

`stop` and `reset` on a disposed controller are teardown no-ops: tearing down
twice is ordinary, not misuse. They report at `debug` on the same target.

The target name, the field names and every `kind` string are a stability
boundary, like an error variant: filters, dashboards and tests select on them.
Renaming one is a breaking change.

A transition receipt (ADR-0202) whose run completed carries `settle_reason`:
`clock` when the run reached its end, or settled at once on a controller
without a clock; `reduced_motion` when the presentation's motion policy
settled it; `exhausted_clock` when the frame clock ran out; `stopped` for an
explicit `finish`. It joins the same stability boundary.

Refusal reporting is not gated by the `diagnostics` feature. A refusal is off
the frame path, rare by definition, and costs one disabled-callsite check
without an interested subscriber.

## Alternatives

A callback hook for refusals would add another delivery protocol beside
`tracing`, as ADR-0202 rejected for observation. Logging at each refusal site
instead of at the command boundary is how refusals were missed: one early `?`
returned without a message. Recording each refused call's own location as well
would add a `#[track_caller]` to every command; the controller's creation site
already says which widget owns it.

## Verification

`refusal_diagnostic_contract` has one row per error kind a public command can
produce and asserts exactly one event with its level, `operation` and `kind`.
With reporting disabled, every row fails. `identity_exhausted` requires 2^64
runs and has no row.
