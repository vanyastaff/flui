# ADR-0175: Scheduling and animation state belong to the UI owner

- **Status:** Accepted
- **Date:** 2026-10-08
- **Supersedes:** ADR-0136's transitional statement that `UpdateScheduler`
  remains `Send`, and its transitional split post-frame queues.
- **Supersedes:** ADR-0064's scheduler ticker surface and explicit-disposal
  cycle, and ADR-0106's scheduler placement and cross-thread sharing of run
  futures. Their controller-owned outcome and continuation custody rules remain.
- **Related:** [ADR-0027](ADR-0027-owner-affine-ui-realms.md),
  [ADR-0136](ADR-0136-owner-local-ui-surfaces.md),
  [ADR-0178](ADR-0178-notification-first-failure-custody.md).

## Context

UI callbacks execute synchronously on their owner. Requiring `Send` captures
forced animation listeners and widget callbacks to share per-node state through
locks. A worker's in-flight frame request also retained the scheduler's UI
callback storage past owner teardown. Providing a second callback lane would
leave callers responsible for choosing storage and interleaving rules.

## Decision

`UpdateScheduler`, animation controllers, Vsync registries and
foundation notification channels share owner state through `Rc`. Mutable owner
state uses `Cell` or `RefCell`; borrows end before user callbacks or outgoing
captures are invoked or retired. Callback and continuation authoring accepts
owner-local captures. Framework notification relays borrow the same failure
custody as their source, as specified by ADR-0178.

Render delegates execute on the UI owner and accept owner-local captures.
`CustomPaint` and `RenderCustomPaint` share their painter through `Rc`, so
indicator painters can observe controller kernels without a cross-thread
trait bound. Callback-free immutable tracks and raster data can retain `Arc`.

An animation run belongs to its controller, and its `AnimationRunFuture`
belongs to the animation layer. The scheduler has no independent ticker,
ticker provider or ticker group. A presentation's frame registry drives its
controllers through typed `FrameTick` values. A manual controller accepts
`Duration`; floating-point seconds cannot enter either frame boundary.
The former `TickerFuture`, `TickerCanceled`, `TickerCompleter` and
`TickerDelivery` become `AnimationRunFuture`, `RunCanceled`, `RunCompleter`
and `RunDelivery`; the latter two are private to the animation layer.

`DrivenController` owns the controller and its registration. Observer clones
share the kernel without keeping its registration alive. The owner unregisters
before disposing the kernel, including during unwind. Moving between registry
identities preserves the last sampled elapsed time. Missing clocks settle
finite runs and park infinite repeats without producing frame demand.

Owner disposal commits kernel closure and cancellation before retiring the
outgoing registry. Cancellation delivery and registry retirement borrow one
first-failure context. An outgoing capture observes a kernel that refuses new
runs; an earlier cancellation failure retains that opaque registry instead of
invoking its destructors. Healthy cancellation continuations still finish.

A presentation also withdraws animation authority independently of widget tree
destruction. `Vsync::prepare_close` permanently closes the registry and attached
descendant identities, withdraws seats and frame routes, and closes their kernels
before returning `VsyncRetirement`. A shared descendant closes for all its
parents; unrelated parent controllers remain live. Saved handles refuse new
registration with `VsyncRegistrationError::Closed`, and a disposed owner cannot
revive its kernel by rebinding. Cancellation outcomes are committed during
preparation; their callbacks and outgoing captures are delivered afterwards.

Whole-runtime close prepares every presentation before its first teardown
callout. Addressed close prepares only that presentation. This remains required
when exceptional close retains an opaque widget tree: retained owners must not
keep an animation run live. Receipt publication borrows enclosing first-failure
custody; explicit finish and drop complete accepted cancellation tails. Incoming
unwind or a previously caught failure retains opaque captures without replacing
the authoritative failure. Healthy close retires them normally.

Rebinding commits the new seat and withdraws the old registration before clock
transition delivery. The outgoing registry remains owned until the kernel has
installed its new clock binding and finished settlement or frame demand. Clock
delivery and outgoing registry retirement borrow one first-failure context;
an outgoing capture cannot interrupt installation of the accepted binding.
After a delivery failure, opaque outgoing ownership follows ADR-0178's retention
policy. `driven_controller_owns_its_seat_and_run` covers outgoing retirement,
competing delivery failures, healthy settlement tails and subsequent runs.

An owner with several controllers stages their migrations through `VsyncUpdate`.
All registrations commit before the first clock callout. `prepare` separates this
commit from `VsyncPublication` delivery without exposing controller borrows;
widgets restore temporarily extracted owner storage before publication. Explicit
publication and drop finish the accepted delivery tail. During unwind, drop
preserves the incoming failure. Switcher and Dismissible consume this contract;
their lifecycle fault boundary may retire the actor after propagation.

`FrameWaker` and task `Waker` remain the cross-thread capabilities. A frame waker
holds a weak reference to wake infrastructure containing the phase, enablement,
request latch, delivery debt and platform hook. It contains no strong reference
to UI callback storage. Closing the scheduler marks that infrastructure closed
before notifying completion waiters. A worker cannot extend the lifetime of UI
state by holding a wake capability or invoking its hook.

There is one post-frame queue. Before the first `OwnerFrame` is constructed,
the scheduler owns its construction queue. The first owner claims that exact
queue; the scheduler then retains only a weak reference. Every `PostFrameHandle`
addresses an exact queue identity. Owner retirement closes admission and
withdraws callbacks before retiring captures. A later owner receives a fresh
queue, and stale handles refuse admission rather than following the replacement.
The existing frame entry points continue to require `&OwnerFrame`.

Data and IO boundaries retain their `Send` requirements. Platform frame hooks,
signal senders, render invalidation handles and callback-free raster data are
independent of owner-local scheduling and animation state.

## Verification and limits

`owner_callback_contract` exercises `Rc` captures through frame, microtask,
priority, persistent and post-frame delivery, including
registration reentry. Its owner-generation row checks construction transfer,
capture retirement on the owner and refusal by stale handles.
`wake_in_flight_releases_owner_storage` pins callback retirement while a frame
hook remains in flight.
Animation's public controller and wrapper failure matrices pin reentry,
logical closure and first-failure custody.
The controller source matrix pins outcome delivery and registration withdrawal
before capture retirement. Mounted implicit widgets and route transitions pin
registry replacement and unmount through their actual lifecycle paths.
`stopping_the_realm_mid_animation_cancels_every_run` checks mounted runtime
shutdown, refusal by every closing kernel inside the first cancellation,
competing cancellation failures, incoming unwind and addressed sibling progress.
`closing_a_registry_withdraws_kernels_before_delivery`, a row of
`driven_controller_owns_its_seat_and_run`, checks nested/shared registry closure,
saved handles, retirement reentry, receipt drop and failure custody.

This decision does not adopt the rest of ADR-0136's draft thread-boundary
ledger, its proposed gate, parallel layout or animation mutation admission
during build. Those require their own implementation and acceptance evidence.

## Alternatives

A second owner-local callback lane duplicates queue ownership and leaves
interleaving to callers. Retaining `Arc<Mutex<_>>` for owner state imposes a
locking protocol without enabling another execution owner. Runtime thread
wrappers turn an ownership mistake into a panic instead of a compile error.
