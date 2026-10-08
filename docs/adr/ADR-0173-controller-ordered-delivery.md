# ADR-0173: Controller transitions and run deliveries preserve commit order

- **Status:** Accepted
- **Date:** 2026-10-08
- **Related:** [ADR-0064](ADR-0064-animation-completion-is-one-controller-resolved-future.md),
  [ADR-0106](ADR-0106-ticker-continuation-ownership-and-terminal-waiters.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md)

## Context

Animation status listeners can reverse a controller, dispose it, or remove another
listener. Recursive delivery allowed a later listener to receive Reverse before
the Completed transition that caused the reversal. A listener failure also
discarded the healthy tail after the transition had already been committed.
Vsync propagated one controller's failure before ticking remaining frame peers.

AnimatedSwitcher and AnimatedSize use controller status to retire outgoing
children and report completion; navigation uses the same controller-resolved
future for transition outcomes. They should not implement their own ordering and
failure recovery around a controller.

## Decision

The controller admits each distinct status transition, its corresponding run
delivery, and displaced resource custody while its state guard is held. The
outermost caller drains this FIFO after releasing the guard. Reentrant mutations
append to that drain; they do not recursively deliver another status. Adjacent
equal statuses are silent, while A to B to A preserves all three transitions.
The committed-status marker records admission, not successful delivery.

A status entry snapshots its subscription membership and callback ownership at
admission. Before invoking each callback, delivery checks that its subscription
still exists and the controller is live. A removed listener or disposed owner is
silent; a subscription added after admission first participates in a subsequent
committed transition. Status subscriptions have no implicit catch-up notification.
The snapshot remains owned through the entire round, including removal by user
code, and retires outside the guard in registration order.

Status notification precedes the associated run delivery. A caught callback or
retirement failure preserves the healthy tail and subsequent admitted entries;
the first failure resumes only after draining and releasing delivery ownership.
Snapshots and displaced resources follow ADR-0127's exceptional retention policy.
Reentrant removals join outgoing custody, including subscriptions added after
the current snapshot. Frame walks and controller ticks borrow the same existing
retirement context, so an earlier peer's failure remains authoritative. A run
delivery after a contained failure uses `TickerDelivery::deliver_after_failure`:
accepted continuations and waiters still run, but their opaque captures and
owning wakers remain retained even on successful invocation. Normal delivery
keeps ordinary destruction.
Published completion cannot become cancellation because a status listener starts
another run. This is ordering of pre-existing continuations in controller run
deliveries: ADR-0064's immediate continuation call when subscribing to an already
resolved future remains unchanged.

Vsync similarly contains each child-registry and controller failure, continues
its existing registration walk, then resumes the first failure. Removed, newly
registered and muted peers retain the existing walk rules. A non-finite frame
instant is ignored before any registry changes its run anchors.

This changes delivery behavior within the existing controller ownership contract.
It does not adopt the draft core ownership migration or change the foundation
notifier's documented logged-listener-failure policy.

## Alternatives

Catch-and-continue alone cannot preserve reentrant transition order. A caller-side
queue makes each widget coordinate the same obligations and cannot protect other
users of its controller. A separate ownership wrapper would duplicate the pending
core migration. Keeping the FIFO in controller state localizes these obligations.

## Verification and limits

Public `status_delivery_contract` exercises reentrant status and run-outcome
ordering, late subscription, A to B to A, and the next frame after a failure.
`status_delivery_failure_custody` runs hostile captures and competing opaque
payloads in a child process, checking the healthy tail and subsequent completion.
Its rows cover removal of a newly registered subscription, continuation and
waiter retirement after status failure, and later run delivery after sibling or
child-registry failure.
The existing status panic, removal, disposal and Vsync sibling/child-registry
tests run in the normal suite. `nan_frame_time_is_skipped` checks anchoring through
public Vsync. These regressions fail with the production changes reverted.

Callbacks run synchronously on the draining caller. While the current controller
remains Send + Sync, a concurrent caller may return after admission before another
caller finishes delivery. This contract is not a global ordering between value
samples and status callbacks, and does not promise recovery from a double panic
inside user code before containment regains control.
