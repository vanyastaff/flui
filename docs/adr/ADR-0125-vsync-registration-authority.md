# ADR-0125: Vsync registration authority and permanent capacity refusal

- **Status:** Accepted
- **Date:** 2026-10-05
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
Tokens are `Clone`, not `Copy`; `unregister` and `detach_child` borrow them.
Foreign, expired, already removed and wrong-kind tokens are harmless. Clones of
the owning registry accept the same token. Dropping a token does not unregister
work; lifecycle owners still remove their registrations explicitly.

Controllers and child registries share one local identity namespace. The maximum
counter value denotes permanent exhaustion and is never issued. Removing work
does not restore capacity. The last admitted slot remains inside the tick fence,
and accepted controllers and children continue advancing after refusal.

`try_register` borrows a controller and reports `VsyncRegistrationError::Exhausted`
without retiring caller-owned values. The existing owned `register` delegates to
it and panics with `Vsync registration capacity exhausted` on exhaustion, after
releasing the registry mutex. Its rejected owner is secured before admission so
ordinary rejected-value destruction cannot replace that failure. Child attachment
continues returning `None` for a cycle or exhaustion. No process-wide allocator
or automatic token removal is introduced.

## Migration and validation

Pass `&registration` to removal methods, and take stored `Option` tokens before
removal when retiring their lifecycle owner. Code requiring recoverable admission
uses `try_register(&controller)`.

`controller_sources_allow_reentry_and_preserve_run_ownership` exercises actual
foreign, stale, expired and wrong-kind tokens and subsequent virtual-clock ticks.
`vsync_nesting_and_reentrancy` seeds only the private counter boundary and then
uses public admission, refusal, removal and tick methods, including mixed final
controller and child admissions and competing rejected-owner retirement.
Whole-registry destruction and a user capture's internal aggregate destructors
remain separate ownership boundaries.
