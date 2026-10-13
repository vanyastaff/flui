# ADR-0219: Observers are the required registration method

- **Status:** Accepted
- **Date:** 2026-10-13
- **Supersedes:** [ADR-0178](ADR-0178-notification-first-failure-custody.md), the default relay
  adapter only; [ADR-0193](ADR-0193-directional-phase-names.md), the `Animation` phase helpers
  only.
- **Related:** [ADR-0180](ADR-0180-source-bound-animation-status-subscriptions.md)

## Context

ADR-0178 let a framework relay borrow the notifying delivery's recovery context
through `Listenable::add_observer` and `Animation::subscribe_status_observer`.
Both were hidden provided methods whose default wrapped the relay in a fresh
recovery. An implementor that wrote only the user-facing method therefore
silently opened a new context at its own boundary: six framework controllers did
exactly that, so a relay such as `AnimatedBuilder` on them saw no earlier
failure and destroyed captures on a failed path. Custom animations had the same
hazard and could not see it, since the method was hidden.

## Decision

- `Listenable::add_observer(ListenerObserver)` and
  `Animation::observe_status(StatusObserver)` are the required methods. They
  receive the notifying delivery's `RecoveryScope`. `add_listener` and
  `subscribe_status` are provided adapters that ignore the scope; a panic in
  them is still contained by the delivery.
- Every implementor decides where its relays run: forward them to the channel
  that notifies, or, for a source that notifies outside a foundation channel,
  open the outermost scope with `RecoveryScope::boundary`.
- `RecoveryScope`, `ListenerObserver`, `StatusObserver`, `RecoveryCallback`,
  `NotificationCallback`, `Notifier::add_with_recovery`,
  `Notifier::notify_with_recovery`, `Notifier::take_callback` and
  `Notifier::inherit_failure`, and the matching `ChangeNotifier` methods are documented public
  API, so an implementor outside the framework needs no hidden
  item. `RecoveryScope`'s classified methods remain framework-internal.
- An animation's phase is asked through `status()`. `Animation` no longer
  provides `is_forward`, `is_reverse`, `is_forward_settled` or
  `is_reverse_settled`; `AnimationStatus` keeps them as `const fn`.

## Consequences

- Out-of-tree `Listenable` and `Animation` implementors must write the observer
  method; callers of `add_listener` and `subscribe_status` are unchanged.
- `relay_failure_custody` (flui-widgets), the Cupertino tab-controller row and
  `a_custom_source_lends_its_delivery_scope_to_wrappers` (flui-animation) pin
  that relays on framework controllers and on custom sources share the
  enclosing failure.
