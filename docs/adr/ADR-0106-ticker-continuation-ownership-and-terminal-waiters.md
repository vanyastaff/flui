# ADR-0106: Ticker continuations own their captures through failure; terminal waiters wake outside locks

- **Status:** Accepted
- **Date:** 2026-10-03
- **Supersedes:** ADR-0064 sections 3 and 6 only for continuation invocation and waiter delivery policy

## Context

Ticker resolution is durable before fan-out. The previous `FnOnce` continuation
bound consumed callback captures during invocation, so a body panic followed by
capture destruction could abort before the executor caught the failure. Secondary
panic payloads were also implicitly destroyed while retaining the first, allowing
aggregate payload destruction to abort before healthy siblings ran.

The event-listener 5.4.2 notifier calls a task's waker under its list lock, forbidding
inline poll or drop. Its intrusive notify implementation advances `next` and marks
an entry notified before `task.wake`, but increments `notified` afterward. A legal
panicking waker leaves inconsistent bookkeeping; retrying notification cannot
reconstruct that count or guarantee safe listener removal.

## Decision

1. `TickerFuture::when_complete_or_cancel` accepts `FnMut`, called exactly once.
   Invocation borrows an owning envelope outside the catch. Pending and
   already-resolved registration use the same boundary; resolved registration
   remains synchronous. Callbacks that consume a capture can use explicit
   `Option::take` inside their body, where that consumption is their responsibility.

2. Preserve the controller-owned completer and publish/deliver separation.
   Publication atomically commits resolution and takes callbacks plus pending
   waiter registrations. The ticker still resolves no controller run itself.

3. Each polled pending future owns one slab index under durable state. Clones
   register independently, repeat polls replace their existing waker and Drop
   removes their registration. Replaced/deregistered wakers retire outside locks.
   Reading terminal state and registering pending work share one critical section,
   so publication cannot lose a waiter between an observation and registration.

4. Invoke callbacks, then `wake_by_ref` every registered waker outside all locks.
   Catch invocation, ordinary owning-envelope retirement and telemetry separately.
   Retain exceptional envelopes and secondary opaque payloads without destroying
   them, preserving the first failure and finishing the healthy fan-out tail.
   After delivery, resume that first payload; during an existing unwind, retain it
   instead. Implicit completer/delivery Drop retains its existing settlement role.

5. Opaque aggregate drop glue cannot be decomposed. Exceptional retention leaks
   captures intentionally; normal successful retirement still runs destructors.
   Two failures arising within one ordinary first aggregate retirement, or inside
   user callback code before it returns to the boundary, remain Rust abort limits.

## Consumers and proof

Navigator push completion and Cupertino release-fade chaining borrow their captured
queues/controllers and satisfy the new bound. Animation controller outcomes and
callback-before-waker order remain unchanged.

The public `ticker_future_delivery_recovery` table covers complete/cancel and
implicit/unwinding delivery, captured aggregates, payload competition, ordinary
retirement, telemetry failure, independent/replaced/dropped waiters, publication
races, inline polling/drop, hostile wakers, healthy tails and the next independent
run. Each case runs in a child process with a timeout and an exact-one-test oracle,
so reverting the ownership or notifier behavior cannot abort or hang the parent.
