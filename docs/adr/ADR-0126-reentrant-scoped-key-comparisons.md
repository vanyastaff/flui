# ADR-0126: Reentrant scoped key comparisons

- **Status:** Accepted
- **Date:** 2026-10-05
- **Supersedes:** [ADR-0043](ADR-0043-presentation-bundled-trees-and-realm-globalkey-scope.md), only its §3 exhaustive outcome claim for custom key callbacks
- **Related:** [ADR-0050](ADR-0050-global-key-identity-and-frame-reservations.md)

## Context

Custom `ViewKey` hashing, equality, cloning and destruction are user code.
They can inspect a saved `GlobalKeyScope` or change another owner sharing it.
Comparing keys under a scope borrow panics on legitimate reentry. Resolving a
scoped release after consuming its local registration can also leave authorities
split if the comparison fails.

## Decision

Hashing precedes scope borrowing. Comparisons pin every key in the relevant
hash bucket, then release the borrow before invoking equality or cloning.
The authority decision validates the complete ordered bucket: its length,
absence, owner tags and allocation markers. An equivalent insertion or a
same-length replacement invalidates the snapshot even when one old marker
survives. Reading the scope or changing an unrelated bucket does not invalidate
it.

Admission and missing-local release permit an initial comparison and one fresh
retry. A second invalidation raises `GlobalKey comparison changed scope
repeatedly` before the requested admission or removal commits. Callback changes
already committed by independent owners remain authoritative. A failed mount is
not promised to be resumable. User hashing and equality can themselves panic or
fail to terminate; this bound limits framework retries, not arbitrary user code.

Successful admission gives the local registration a passive identity containing
cached hash, owner tag and allocation marker. Matching release withdraws local
and exact scoped authority without scoped key callbacks, before local-then-scoped
key retirement. A stale identity cannot remove a replacement claim. Same-owner
admission and intra-tree retake keep the existing scope claim. Missing-local
release uses the bounded comparison policy and remains owner checked.

Rollback ownership exists before post-admission snapshot retirement. Each scope
key and comparison pin has its own guarded ownership envelope. Healthy
destruction remains observable; a live snapshot can postpone physical key
destruction after logical withdrawal. Snapshot retirement preserves the first
caught failure explicitly and retains later independent envelopes. An incoming
unwind likewise retains those envelopes. This does not rescue a healthy opaque
user aggregate whose internal destructors already double-panic.

ADR-0043's presentation topology, claim lifetime, uniqueness and retake decisions
remain in force. Its three ownership-interleaving outcomes do not exhaust custom
callback failures: repeated comparison mutation now has an explicit bounded
refusal, and ordinary user callback failures propagate through the existing
containment policy.

## Validation

The public `owner_key_lookup_reentry` rows join
`lifecycle_panic_containment_matrix`. They cover read reentry, relevant and
unrelated bucket mutation, equivalent insertion, same-length replacement,
bounded refusal, callback failures, first and later snapshot retirement,
independent progress, passive release, retake, stale ownership and a mounted
keyed `DenseRow` through normal build and finalization.
