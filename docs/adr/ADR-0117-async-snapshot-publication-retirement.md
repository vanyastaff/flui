# ADR-0117: Async snapshots publish before retiring generic values

- **Status:** Accepted (2026-10-04)
- **Date:** 2026-10-04
- **Superseded-by:** [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md),
  only the sentence retaining a competing wake payload without running its destructor: the
  payload is discarded under containment (`discard_payload`).
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
under its subscription-generation fence or changes its enum variant while
preserving the last `Result`. The old snapshot retires after unlocking.
Initial-data factories likewise run outside the slot Mutex.

Accepted replacement publication precedes old-value destruction. A caught old
retirement failure still attempts the matching rebuild and host delivery before
resuming its original payload. A competing wake payload is discarded under
containment (`discard_payload`). Stale incoming snapshots retire outside the
lock and request no rebuild. Inline future completion still suppresses a
redundant rebuild, preserves the existing Done guard, and uses the original
keyed identity and result-preserving variant transitions.

The borrowed caller Arc remains alive through the contained retirement and wake
calls. After an accepted-update failure the first failure resumes and the slot
keeps no additional owner. Removing the failed producer or eagerly initialized
state therefore destroys the published incoming value with its ordinary owners;
there is no separate retained slot guard after failure.

The scheduler still terminates a task whose poll panics. Recovery means that an
already requested build can observe the published snapshot and a fresh keyed
subscription can publish again. It does not mean continued polling of that task.
An ordinary first aggregate destructor with several panicking fields can abort
before containment regains control. Arbitrary user poll/build failures and
ordinary final-state disposal retain their existing policies.

No new executor is introduced. ADR-0018's public `flui-view` snapshot enum,
keyed subscription, borrowed snapshot and frame delivery decisions remain in
force; this decision supplies their retirement failure ordering.

## Verification

FutureBuilder and StreamBuilder rows in `lifecycle_panic_containment_matrix`
exercise old retirement alone, competing old/new values, retirement plus wake
failure, and real eager state disposal. Delayed producer rows observe the
committed incoming value on a build without test-induced dirtiness, then replace
the key, observe `Waiting` with the preserved result, and observe another
completion. Bounded children contain counterfactual process aborts. Final
disposal checks that no additional slot owner keeps the incoming value alive.
