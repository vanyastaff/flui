# ADR-0180: Animation status subscriptions own source-bound removal

- **Status:** Accepted
- **Date:** 2026-10-09
- **Related:** [ADR-0177](ADR-0177-controller-ordered-delivery.md),
  [ADR-0178](ADR-0178-notification-first-failure-custody.md),
  [ADR-0179](ADR-0179-controller-registration-ownership.md)

## Context

A listener ID names a slot inside a notification channel. An ID alone cannot
identify its channel: independent animations may issue the same numeric value.
Accepting it through an arbitrary animation's removal method lets callers remove
an unrelated callback. Parent replacement and switch hops also make callers
responsible for remembering which source admitted each registration.

Status callbacks may reenter their source during delivery or capture destruction.
Replacing the public API must preserve ordered delivery, callback membership and
the shared first-failure custody policy.

## Decision

`Animation<T>::subscribe_status` returns a non-cloneable, must-use
`StatusSubscription`. Dropping the subscription withdraws its own registration.
`detach` transfers callback lifetime to the source without invoking removal.
There is no public status ID or source-selected status removal method. Value
notification keeps the separate `Listenable` contract.

The guard holds a weak reference to the logical source owner, a copyable
registration token and a removal function pointer. It cannot keep the animation
owner alive and contains no user captures. Custom sources construct the same
guard; they must reject stale tokens if they reuse storage. The removal function
commits withdrawal and returns outgoing callback custody after releasing state
guards. Cancellation releases its temporary strong source reference before
retiring that custody through the borrowed recovery context. A capture destructor
that releases the last logical owner therefore closes its channels before
reentrant notification. Deferred delivery custody stays in the source's existing
queue and the removal function returns an empty value.

Reverse, curved and tween animations bind removal to their shared parent-links
owner. Proxy and switch animations bind it to their own owners. Their observers
remain subscribed across parent replacement and active-parent hops. The last
wrapper clone closes its channels even when a guard survives. A temporary relay
reference to a notifier does not count as a surviving wrapper owner.

Framework status relays borrow the enclosing delivery's `PanicRecovery`.
Internal parent guards and higher-layer cleanup use `cancel_with_recovery` when
they belong to an existing retirement round. Removal commits membership changes
before callback destruction. Controller and proxy removal during delivery use
their existing deferred retirement storage, without a second notification queue.
First-failure retention and last-owner tail silence follow ADR-0178.

Consumers that need earlier removal retain the guard alongside their source.
Callbacks that are deliberately owned for a controller's complete lifetime
detach the guard; retiring that controller closes their registrations.

## Evidence

`owning_status_subscription_contract` covers independent sources and channels,
self and later removal during delivery, detach, surviving guards after source
teardown, custom source construction, capture reentry, failure custody and
subsequent progress. Its wrapper rows cover shared clone lifetime, proxy parent
replacement and an actual switch hop. Its enclosing-cleanup row verifies that
cancellation inherits an already established failure. Its capture-retirement
row releases the last wrapper owner and reenters the parent; the detached tail
must remain silent for reverse, curved, tween, proxy and switch sources.

`thread_boundary_ui` rejects
`status_removal_requires_owning_subscription`. Restoring the former manual
controller methods makes that fixture compile, so the compile-fail suite detects
the return of foreign-source removal authority. Runtime delivery and retirement
remain covered by `status_delivery_contract`, `status_delivery_failure_custody`
and the mounted widget and Material contract tables.
