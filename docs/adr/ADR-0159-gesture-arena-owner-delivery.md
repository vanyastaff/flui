# ADR-0159: Gesture arena delivery belongs to the input owner

- **Status:** Accepted
- **Date:** 2026-10-07
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)

The binding and its recognizers share one owner-local gesture arena. Its
storage does not synchronize independent threads. Every state transition
finishes before member callbacks or user destructors run, so either can
reenter the same arena without an outstanding storage borrow.

An entry identifies an exact arena generation. Pointer-up may detach a held
generation while a later contact reuses the pointer identity; releasing or
resolving the earlier entry affects only that generation. Deferred default
resolutions carry the same identity and cannot resolve a replacement contest.
Generation and registration counters refuse permanently at exhaustion.

Frame deadline polling visits active slots in ascending pointer identity
order, then held slots in generation order, then explicit timer registrations
in registration order. A live member shared by these sources is polled once.
This stable order makes simultaneous callback effects independent of storage
layout; timer registration still outlives arena resolution until its owner
retires it. Storage types and strong versus weak member retention are separate
implementation choices.

Resolution removes the exact slot before notification delivery. Explicit
resolution rejects losers in registration order before accepting the winner;
a pointer-up sweep accepts the first member before rejecting later members.
A failing candidate destructor or notification does not discard later
notifications. The first failure propagates after delivery, and remaining
user ownership follows ADR-0127 retention. This containment cannot rescue a
single aggregate whose fields double-panic before reaching the boundary.

The public `gesture_lifecycle_matrix` covers candidate destructor reentry,
competing retirement failures and recovery, and deadline order.
`arena_settles_every_member_exactly_once` covers held generations, deferred
resolution, stale entries and pointer reuse.
