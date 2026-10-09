# ADR-0178: Notification delivery preserves the enclosing first failure

- **Status:** Accepted
- **Date:** 2026-10-08
- **Supersedes:** [ADR-0109](ADR-0109-notification-channel-surface.md), decisions 3–4's
  swallowed listener failure and snapshot disposal behavior.
- **Related:** [ADR-0119](ADR-0119-inert-panic-payload-retirement.md),
  [ADR-0127](ADR-0127-exceptional-path-retention.md),
  [ADR-0177](ADR-0177-controller-ordered-delivery.md)

## Context

Animation value delivery uses the foundation notification channel. Swallowing a
value-listener panic loses the frame's first failure, while disposing a channel
and continuing its old snapshot invokes subscriptions whose owner has retired.
A healthy tail can also add and immediately remove an opaque capture after a
failure was caught. At that point `thread::panicking()` is false, so treating
retirement as healthy can introduce another failure or abort.

## Decision

`Notifier` owns a registration-ordered snapshot for each synchronous notification.
Notification channels and their callbacks are owner-local `Rc` values; internal
state uses `Cell` and `RefCell`. Worker wake capabilities remain separately
`Send + Sync` and do not retain channel storage.
It rechecks live membership and disposal before each invocation. Removal or
disposal withdraws admission immediately; additions first participate in a later
round. Arguments remain borrowed: neither `Clone` nor a static argument type is
required. Reentrant generic notifications may borrow a different argument, so
the generic channel does not invent an owned argument queue.

The first listener failure is captured before diagnostics. Delivery continues
through healthy live listeners, then resumes that failure. Subsequent callback,
diagnostic and retirement failures cannot replace it. Exact text payloads may
retire normally when superseded; opaque payloads retain ADR-0119's policy.

The foundation `panic::PanicRecovery` owns the first failure and alone completes
delivery. Its `RecoveryScope` lends that custody without owning a payload or a
completion capability. Animation controller delivery and Vsync borrow it through their actual
call paths. A notifier entered during an earlier frame failure inherits that
custody. Channel-local delivery depth and a failure latch cover nested calls
through independent handles to the same channel; no production ambient recovery
registry or process-global failure flag is introduced. Each active channel lends
its failure signal to the enclosing recovery for its whole notification and
retirement scope. Capturing or inheriting a failure marks every active signal
immediately, before diagnostics or another callback can reenter ancestor cleanup.
Only the outermost delivery of a channel clears its signal. The scoped signals
retain neither channel storage nor owners and preserve the recovery's `Send`
capability. Each link borrows its channel's atomic signal and the enclosing
stack-local link. Nesting does not grow a heap collection; the borrow checker
prevents a scope or signal reference from escaping its synchronous delivery.

Framework relays borrow this same context through `Listenable::add_observer`
and `Animation::subscribe_status_observer`. User listeners retain their ordinary
callback signature. Both kinds share the channel's membership, ordering and
retirement storage. Foundation channels, controllers and combinators override
the relay seams so a chain of wrappers does not start a fresh recovery after
an earlier parent failure. A custom listenable using the default relay adapter
starts a new context at its own notification boundary; custom channels that
provide containment must override the seam to propagate their context.

Reentrant owner cleanup can inherit an active channel's failure latch without
borrowing its storage across user code. No payload is copied: the original
enclosing context remains responsible for resuming the first failure. Proxy
swaps admit queued value and status notifications with committed status
membership; reentrant swaps append so the last committed parent's status is
the last announced status.

Wrapper lifetime belongs to a shared owner allocation, independently of the
temporary storage references a notification needs. Final owner destruction
withdraws channel membership before parent detachments and capture retirement.
Reverse, curved and tween wrappers share the same parent-link owner. Switch
dispatch borrows separate state storage, so a callback cannot accidentally
extend the wrapper's owning lifetime. A last owner released by its own listener
silences the rest of that round, including during an earlier parent failure.

During a notification, outgoing callback ownership waits until the outermost
round retires it. Snapshot and outgoing captures retire outside state borrows,
in registration order, while the failure context is still authoritative. Healthy
destruction is preserved. After a caught failure, opaque outgoing ownership is
retained instead of running arbitrary destructors.

`ChangeNotifier` exposes hidden custody-transfer methods for framework callers:
withdraw one listener, all listeners, or dispose and withdraw them. The caller
must own the returned callbacks until its enclosing recovery retires them.
Animation uses these methods to commit value-listener removal and disposal
immediately while placing outgoing custody into its existing delivery FIFO.
Disposed controllers refuse later mutations and listener admission.

Ticker start and stop also join that FIFO. The run and its completer are committed
before a platform wake hook runs, and no controller borrow is held across the
hook. A stale queued start checks the installed run's generation and liveness.
The ticker callback holds weak controller state and notifier references, so an
active ticker cannot retain the controller's last owner. Last-owner destruction
withdraws resources and cancels the run before retiring captures.

## Verification and limits

The public `notifier_ownership_and_recovery` family checks failure propagation,
healthy tails, disposed snapshots, late capture removal, borrowed arguments,
non-Clone values, hostile payloads, diagnostic competition and terminal recovery.
Its recursive relay case checks that nested completion cannot clear the outer
channel's failure before a newly admitted capture is withdrawn and retired.
`status_delivery_failure_custody` includes value capture retirement after a
status, value or earlier peer failure and verifies the next frame still delivers.
It also covers value and status forwarding through reverse, curved, tween,
proxy and switch wrappers, and an independently nested wrapper chain.
Late parent capture retirement is checked both after a child relay returns and
inside the child's healthy tail, with single and competing callback failures.
The same family verifies last-owner release from both notification channels,
healthy and failed tails, and the next parent notification.
The tests fail when the production behavior is reverted.

`a_steady_state_frame_allocates_nothing` also measures a running controller
forwarding through five reverse wrappers to a live leaf listener. Every measured
frame reaches the leaf and advances its value, with no allocations. The original
delivery code passes this case; a heap-spilling recovery stack fails it.

The public hook read, registry query, stop/replace/dispose and last-controller
drop tests pin ticker admission and lifetime. Callback failures do not change
already-published completion or cancellation outcomes.

Containment starts once control returns from user code. A callback or aggregate
whose destructors double-panic before reaching containment can still abort under
Rust's rules. This record does not adopt every surface or gate in the draft
owner-local UI migration.
