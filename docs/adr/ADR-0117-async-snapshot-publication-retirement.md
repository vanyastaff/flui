# ADR-0117: Async snapshots publish before retiring generic values

- **Status:** Accepted
- **Date:** 2026-10-04
- **Related:** ADR-0018 (async builders), ADR-0109 (notification ownership)

## Context

A consuming snapshot transition dropped the previous generic data or error
while holding the shared slot Mutex. If that destructor panicked, the incoming
value had not been published, the slot held an empty placeholder, and no rebuild
was requested. Destruction of the incoming value during the same unwind could
abort the process. This left ADR-0018's publication-before-rebuild contract
without a defined retirement failure order.

## Decision

Future and stream producers construct replacement snapshots before acquiring
the slot lock. A closed internal update vocabulary either replaces the snapshot
under its subscription-generation fence or changes connection state while
preserving its payload. The old snapshot retires after unlocking. Initial-data
factories likewise run outside the slot Mutex.

Accepted replacement publication precedes old-value destruction. A caught old
retirement failure still attempts the matching rebuild and host delivery before
resuming its original payload. A competing wake payload is retained without
running its opaque destructor. Stale incoming snapshots retire outside the lock
and request no rebuild. Inline future completion still suppresses a redundant
rebuild, preserves the existing Done guard, and uses the original keyed identity
and data/error transitions.

The borrowed caller Arc remains alive through the contained retirement and wake
calls. Before an accepted-update failure escapes, the writer clones and retains
one owning slot Arc. Removing the failed producer or eagerly initialized state
therefore cannot retire the published incoming value in competition. Healthy
updates incur no additional guard clone. This retains the slot's lifetime, not
an immutable historical value across later writes.

The scheduler still terminates a task whose poll panics. Recovery means that an
already requested build can observe the published snapshot and a fresh keyed
subscription can publish again. It does not mean continued polling of that task.
An ordinary first aggregate destructor with several panicking fields can abort
before containment regains control. Arbitrary user poll/build failures and
ordinary final-state disposal retain their existing policies.

No public foundation snapshot API or new executor is introduced. ADR-0018's
keyed subscription, borrowed snapshot and frame delivery decisions remain in
force; this decision supplies their retirement failure ordering.

## Verification

FutureBuilder and StreamBuilder rows in `lifecycle_panic_containment_matrix`
exercise old retirement alone, competing old/new values, retirement plus wake
failure, and real eager state disposal. Delayed producer rows observe the
committed incoming value on a build without test-induced dirtiness, then replace
the key and observe another completion. Bounded children contain counterfactual
process aborts.

The private `accepted_publication_guard_retains_incoming_after_caller_disposal`
row in `future_builder_matrix` calls the same production writer with a sole
caller-owned Arc. It distinguishes exceptional guard retention from independent
scheduler ownership of a failed task. The eager public row is a separate builder
contract and is not claimed as a guard-only oracle.
