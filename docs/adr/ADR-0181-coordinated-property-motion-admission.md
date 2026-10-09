# ADR-0181: Property motion is admitted as a coordinated update

- **Status:** Accepted
- **Date:** 2026-10-09
- **Related:** [ADR-0176](ADR-0176-presentation-animation-playback.md),
  [ADR-0178](ADR-0178-notification-first-failure-custody.md),
  [ADR-0179](ADR-0179-controller-registration-ownership.md)

## Context

Independent property owners preserve unchanged trajectories and deadlines.
Sequential retargeting, however, can admit a width replacement before a height
curve refuses preparation. Resetting matrix progress before preparing its next
run similarly cancels an accepted trajectory when the next curve is invalid.
Rolling back fields cannot undo delivered cancellations, callbacks or wakeups.

## Decision

`MotionUpdate` prepares independently owned values under exclusive borrows.
It retains the actual segments produced by converters and curves; admission
does not evaluate user motion a second time. After preparation, every sampled
identity is validated before any owner changes. Later converter reentry that
invalidates an earlier sample refuses the whole update.

Installation moves prepared framework fields and commits controller delivery
debt without invoking or destroying user code. Every participating owner is
installed before publication. The admitted callback can publish associated
consumer state outside borrows, then controller deliveries drain through one
borrowed `RecoveryScope`. A failed callback does not erase accepted delivery.
The enclosing `PanicRecovery` propagates the first failure after cleanup.

Preparation refusal poisons the update even if its error is ignored. A panic
caught inside the preparing closure leaves that update poisoned too; it cannot
admit an earlier prefix. Uncaught preparation panic leaves old runs installed.
Outgoing and rejected owners commit logical closure and unregister their seats
before opaque captures retire. First-failure retention may keep captures alive,
but cannot keep a rejected or removed owner registered and animating.

Optional properties stage owner insertion or removal in the same update.
Present-to-present motion preserves incoming velocity; appearing and disappearing
properties have no interpolation endpoint and snap. Removal commits all affected
controller closures before cancellation callbacks can observe the group.

Matrix interpolation keeps its decomposition tween and guarantees position
continuity. It stages scalar progress from a resting origin in one admission,
then publishes the reanchored matrix before motion delivery. It does not promise
matrix velocity continuity. Numeric and color properties keep independent
trajectories and deadlines. This coordination applies to admission; individual
owners retain the presentation's existing frame sampling order.

A single composite property vector would couple optional endpoints and deadlines.
Per-property rollback would expose transient runs and irrevocable callouts.
Preparation and coordinated installation retain the existing ownership model
while removing those failure modes. Admission may allocate; steady-state frame
sampling retains the existing allocation-free contract.

## Evidence

`owning_animated_value_contract` checks all-owner visibility before cancellation
and value delivery, ignored refusal, caught preparation panic, cross-owner
converter reentry, rejected optional-owner registration cleanup and grouped
removal with cancellation failure and accepted tail delivery.

`animation_and_visibility` mounts
`container_refused_property_motion_preserves_the_admitted_goals` and
`container_refused_transform_motion_preserves_the_admitted_matrix`, including
refusal after numeric preparation succeeds. They inspect actual laid-out size
and transform layers, subsequent recovery and unmount registration cleanup.
