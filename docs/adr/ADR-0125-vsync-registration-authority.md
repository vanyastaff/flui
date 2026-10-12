# ADR-0125: Vsync registration authority and permanent capacity refusal

- **Status:** Accepted
- **Superseded-by:** [ADR-0179](ADR-0179-controller-registration-ownership.md),
  public controller admission and explicit caller-managed controller removal only.
  [ADR-0191](ADR-0191-single-parent-animation-registries.md) supersedes child attachment refusal and topology.
- **Date:** 2026-10-05
- **Amended:** 2026-10-11 — current driven ownership is distinguished from the historical manual-registration API; identity and exhaustion rules remain in force.
- **Supersedes:** ADR-0020 §1, registration token identity and removal signatures only

## Context

Independent frame registries can issue the same local slot. A slot alone cannot
authorize removal: a token from another registry must not remove an unrelated
controller or child. Wrapping a counter would also let a stale token name newly
accepted work and break the exclusive fence used by the tick walk.

## Decision

`VsyncRegistration` carries weak backend ownership and a monotonically issued
local slot. Equality and hashing use that backend identity and slot. A weak token
reserves the allocation identity without retaining the registry or its controllers.
Tokens are `Clone`, not `Copy`; internal `unregister` and public `detach_child` borrow them.
Foreign, expired, already removed and wrong-kind tokens are harmless. Clones of
the owning registry accept the same token. Dropping a token does not unregister
work. Child owners still detach explicitly; controller seats are withdrawn by
their `DrivenController` owner under ADR-0179.

Controllers and child registries share one local identity namespace. The maximum
counter value denotes permanent exhaustion and is never issued. Removing work
does not restore capacity. The last admitted slot remains inside the tick fence,
and accepted controllers and children continue advancing after refusal.

The original manual-registration API described below is superseded by ADR-0179.
Controller admission/removal is now internal and owned by `DrivenController`;
recoverable public controller migration uses that owner's `rebind` operation.
The original rejection-custody rule remains part of the internal admission path.

Historically, `try_register` borrowed a controller and reported `VsyncRegistrationError::Exhausted`
without retiring caller-owned values. The existing owned `register` delegates to
it and panics with `Vsync registration capacity exhausted` on exhaustion, after
releasing the registry borrow. Its rejected owner is secured before admission so
ordinary rejected-value destruction cannot replace that failure. Child attachment
now returns typed refusals under ADR-0191. No process-wide allocator
or automatic token removal is introduced.

## Migration and validation

Child callers pass `&registration` to detachment and take stored `Option` tokens
before retiring their lifecycle owner. Public controllers use `build_on` and
`DrivenController::rebind` under ADR-0179. The former public
`try_register(&controller)` migration is historical, not a second admission surface.

`registration_identity_and_retirement` exercises actual
foreign, stale, expired and wrong-kind tokens and subsequent virtual-clock ticks.
`vsync_nesting_and_reentrancy` seeds only the private counter boundary and then
uses private admission/removal and public tick methods, including mixed final
controller and child admissions and competing rejected-owner retirement.
Whole-registry destruction and a user capture's internal aggregate destructors
remain separate ownership boundaries.
