# ADR-0248: Leaving `Detached` enters the lifecycle from the `Paused` side

- **Status:** Accepted
- **Date:** 2026-10-19
- **Amends:** [ADR-0035](ADR-0035-lifecycle-consolidation-and-frames-enabled.md) §4 (the ladder
  out of `Detached`)

## Context

ADR-0035 §4 has a presentation's local lifecycle steps follow `lifecycle_ladder(old, new)`, which
walks the declaration order `Detached, Resumed, Inactive, Hidden, Paused`. Because `Detached` is
first, leaving it always passes `Resumed`: `Detached -> Inactive` is `[Resumed, Inactive]` and
`Detached -> Paused` is `[Resumed, Inactive, Hidden, Paused]`.

That is reachable on every launch where a window starts with its execution state `Detached` and
comes up unfocused or in the background. On iOS the bootstrap snapshot commits `Detached`, then
`sceneWillEnterForeground` makes the target `Inactive`, and widget observers and lifecycle
subscribers see `Resumed` and then `Inactive` before the scene becomes active. An app that starts
the camera, audio or a sync on `Resumed` and stops it on `Inactive` starts and stops it for
nothing. Leaving goes the other way round: `Paused -> Detached` is the last step of the ladder, so
entry and exit were asymmetric.

## Decision

The ladder is a ring, and a presentation enters it on the side it leaves by. From `Detached` it
climbs `Paused, Hidden, Inactive, Resumed` and stops at the target:

| From `Detached` to | Steps |
|--------------------|-------|
| `Paused` | `[Paused]` |
| `Hidden` | `[Paused, Hidden]` |
| `Inactive` | `[Paused, Hidden, Inactive]` |
| `Resumed` | `[Paused, Hidden, Inactive, Resumed]` |

No step out of `Detached` is more active than the state entered. Every other transition, and
every transition into `Detached`, is unchanged. The scheduler aggregate is computed from live
facts, not from the ladder, and is not affected.

## Consequences

- A window that starts detached reports only states it is at least as active as.
- `Detached -> Resumed` is now four steps instead of one: an observer of `Paused`, `Hidden` or
  `Inactive` hears each once on such a launch, before `Resumed`.
- `lifecycle_derivation_matrix` pins every `Detached -> X` ladder.
