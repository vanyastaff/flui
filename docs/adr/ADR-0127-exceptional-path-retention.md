# ADR-0127: Retain user-owned values on exceptional paths

- **Status:** Accepted
- **Date:** 2026-10-05
- **Supersedes:** [ADR-0119](ADR-0119-inert-panic-payload-retirement.md), in part:
  its statement that callback captures, queued obligations and render-object
  destruction keep their existing ownership policies. Its payload rule stands.
- **Related:** [ADR-0104](ADR-0104-borrowed-notification-and-opaque-panic-retention.md)

## Context

Framework containers own values whose destructors run user code: listener and
callback captures, route and overlay entries, render objects, futures, keys and
text stores. Rust aborts the process when a destructor panics while the thread
is already unwinding, and a container that keeps dropping its remaining values
after one of them panicked turns a contained failure into a second one.
ADR-0104 already retains callback snapshots and opaque payloads after a caught
failure; other containers dropped their contents unconditionally, so one
hostile destructor could abort a realm that frame containment would otherwise
have kept alive.

## Decision

1. A framework container withdraws a value from its storage, releases every
   borrow and lock, and only then runs the value's destructor.
2. Once a retirement in the same operation has failed, or while the thread is
   already panicking, the container retains the remaining user-owned values
   instead of dropping them (`std::mem::forget`). The first failure propagates;
   later failures are retained without replacing it.
3. Retention applies only where dropping would run user code. Dropping a
   reference-counted clone that is not the last owner, a framework-owned
   handle (a platform window, an accessibility bridge, a GPU resource owned by
   the engine) or a known inert payload (ADR-0119) is ordinary destruction.
4. A retained value is a deliberate leak. The container does not report it
   separately; the failure that caused it is reported where it is caught.

## Consequences

Destructors of retained values never run: a channel sender held by a retained
capture is never closed, and a retained render object keeps its resources until
process exit. This is the cost of keeping the process alive after a destructor
failure, and it is bounded to operations that already failed. Healthy
destruction is unchanged and keeps container order.

Tests that pin retention run in a child process, since the alternative they
guard against is an abort. They assert the first failure, the retained tail and
the next healthy operation on the same owner.
