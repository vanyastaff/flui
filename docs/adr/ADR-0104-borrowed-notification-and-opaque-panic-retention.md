# ADR-0104: Borrowed notification arguments and opaque panic retention

- **Status:** Accepted.
- **Superseded-by:** [ADR-0109](ADR-0109-notification-channel-surface.md)
- **Date:** 2026-10-03
- **Related:** [ADR-0074](ADR-0074-realm-scoped-signals.md), [ADR-0085](ADR-0085-reactive-core-placement-and-phase-subscribers.md)

## Context

Foundation notification channels serve public listener registries and the
zero-argument ChangeNotifier surface used by framework controllers. A callback
previously received a cloned argument by value. If it panicked, destruction of
that owned argument could panic during the same unwind, aborting before the
notification boundary regained control. A caught panic payload can itself own
several hostile destructors; attempting to drop it inside another catch does
not prevent an aggregate's second field from aborting the process.

## Decision

1. `ArgCallback<T>` borrows `&T`. `Notifier::notify` and
   `ListenerRegistry::notify_status` accept borrowed arguments, require no
   `Clone`, and never own an argument's destruction. `Notifier` clones share
   the channel even when its argument type does not implement `Clone`.
   The zero-argument `Listenable` and `ChangeNotifier` APIs remain unchanged.
2. A caught listener failure is reported and later live listeners continue.
   Its opaque panic payload and the callback snapshot are deliberately retained.
   Reporting borrows that payload behind a separate unwind boundary; a subscriber
   failure is retained too, so diagnostics cannot interrupt the remaining listeners.
   A self-removing callback may leave the snapshot owning its last envelope;
   running opaque capture destruction after containment would introduce another
   failure. The exceptional path leaks these obligations instead.
3. Successful rounds retire snapshot envelopes in registration order. If one
   envelope's destructor panics, remaining envelopes are retained and that first
   retirement failure propagates. No notifier lock is held during retirement.
   A single envelope whose own aggregate fields double-panic during ordinary
   destruction, or double-panic within user callback code, retains ordinary
   Rust abort semantics. The boundary promises containment of failures it
   catches, not recovery from an abort before it regains control.
4. Foundation's `panic::retain_opaque_payload` is the shared operation for
   discarded exceptional payloads. It always retains them; it does not attempt
   arbitrary drop glue. Propagated original failures use `resume_unwind`.

## Consequences and verification

This changes the public typed-listener contract and its foundation registry
wrapper. Consumers retaining an argument clone explicitly inside their callback;
notification itself works with non-Clone values. Animation's separate Copy
status callback API is not this generic channel and does not change.

`notifier_ownership_and_recovery` exercises the public API in isolated child
processes: non-Clone arguments, single and aggregate hostile payloads,
self-removal with hostile capture and payload aggregates, a competing tracing
subscriber failure, ordinary competing
retirement failures, later-listener execution, and the next notification after
containment. Existing notification tables preserve ordering, mid-round removal
and disposal behavior. The foundation test-table runner also retains opaque
payloads rather than trying repeated destructor retirement.
