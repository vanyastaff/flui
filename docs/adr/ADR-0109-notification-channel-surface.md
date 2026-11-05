# ADR-0109: Notification channels without an unused registry wrapper

- **Status:** Accepted (2026-10-04)
- **Superseded-by:** [ADR-0178](ADR-0178-notification-first-failure-custody.md)
  for decisions 3–4's listener failure propagation and disposal behavior.
- **Superseded-by:** [ADR-0119](ADR-0119-inert-panic-payload-retirement.md),
  only the known inert text-payload retirement policy in decisions 3–4.
- **Superseded-by:** [ADR-0281](ADR-0281-a-destructor-that-panics-during-an-unwind-aborts-as-in-std.md),
  decisions 3–4 only where they retain the failed snapshot, the remaining envelopes and a
  discarded payload: `panic::retain_opaque_payload` is deleted, a discarded payload goes to
  `discard_payload`, and the rest is destroyed under containment.
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
5. `Listenable`'s registration is an owned subscription. The trait carries two
   required methods, `listen` and `listen_with_recovery`
   ([ADR-0219](ADR-0219-required-observer-registration.md) names the relay's
   recovery custody); each returns a `Subscription`, `#[must_use]` and
   `!Send`, whose drop detaches through its own channel — or, for a custom
   source whose storage is no foundation listener set, through the removal the
   subscription was built with (`Subscription::for_removal`, the twin of
   ADR-0180's status guard's removal function). An inert source answers
   `Subscription::inert()`. A listener id never leaves the `Listenable` trait:
   the id is the channel's own identity, its removal machinery the concrete
   channel's (`ChangeNotifier::take_listener`, `take_listeners`,
   `Notifier::add`/`add_with_recovery`), and the concrete owner clears its
   channel with `dispose`. Sources that refuse a registration hand back the
   inert form; an unmounted or closed source's listen checks its gate before
   the channel's disposed state can answer.

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
