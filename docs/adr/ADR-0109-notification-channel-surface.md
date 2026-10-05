# ADR-0109: Notification channels without an unused registry wrapper

- **Status:** Accepted.
- **Superseded-by:** [ADR-0119](ADR-0119-inert-panic-payload-retirement.md),
  only the known inert text-payload retirement policy in decisions 3–4.
- **Date:** 2026-10-04
- **Supersedes:** [ADR-0104](ADR-0104-borrowed-notification-and-opaque-panic-retention.md)

## Context

The typed notification channel is used by `ValueNotifier` and the zero-argument
`ChangeNotifier` channel. The separately exported `ListenerRegistry` wrapper has
no production consumer. Keeping another registry surface duplicates listener
ownership and recovery obligations without expressing a distinct contract.
`ValueNotifier` also unnecessarily required `Clone` for ordinary operations that
move or borrow its value.

## Decision

1. Remove `ListenerRegistry`. Use the existing `Notifier` channel for borrowed
   typed notifications, and `ChangeNotifier` for zero-argument listeners.
   `ArgCallback<T>` receives `&T`; `Notifier::notify` borrows its argument and
   never owns its destruction. Channel clones share listeners without requiring
   `T: Clone`.
2. `ValueNotifier<T>` admits non-Clone values for construction, access, mutation
   and extraction. Cloning a value notifier still requires `T: Clone`: the
   cloned values are independent and the listener channel is shared. Extracting
   a value disposes that channel, as before.
3. Retain ADR-0104's recovery policy. A caught listener failure and its callback
   snapshot are deliberately retained; reporting borrows the payload behind a
   separate unwind boundary, and a reporting failure is retained too. Later
   live listeners and subsequent notifications continue.
4. Successful rounds retire snapshots in registration order, outside locks. If
   retirement panics, retain the remaining envelopes and propagate the first
   retirement failure. `panic::retain_opaque_payload` deliberately retains a
   discarded opaque payload; a propagated original failure uses `resume_unwind`.

ADR-0119 refines discarded-payload retention: exact static-string and owned-string
payloads are released; all opaque payloads and exceptional callback snapshots
retain the policy above.

## Consequences and verification

This removes an unused public wrapper and broadens the ordinary value-notifier
API. `notifier_ownership_and_recovery` exercises non-Clone values through public
operations, clone compatibility, shared-channel disposal, and isolated child
processes for hostile payloads, callback capture retirement and competing
diagnostic failures. Existing listener tables preserve ordering, removal and
disposal behavior.

Containment starts after the boundary regains control. Competing panics inside
user callback locals or ordinary aggregate destruction can abort before that
point; this policy does not promise recovery from those aborts.
