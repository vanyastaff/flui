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
hostile destructor could abort a UI runtime that frame containment would otherwise
have kept alive.

## Decision

1. A framework container withdraws a value from its storage, releases every
   borrow and lock, and only then runs the value's destructor.
2. Once any failure in the operation has been caught (a callback, a user
   destructor or any other user code it ran), or while the thread is already
   panicking, the container retains the remaining user-owned values instead of
   dropping them (`std::mem::forget`). This includes the snapshot or envelope
   whose callback failed. The first failure propagates; later failures are
   retained without replacing it.
3. Retention applies only where dropping would run user code. Dropping a
   single-threaded `Rc` clone that is not the last owner, a framework-owned
   handle (a platform window, an accessibility bridge, a GPU resource owned by
   the engine) or a known inert payload (ADR-0119) is ordinary destruction. A
   thread-shared `Arc` clone is retained: another thread can release its own
   clone after any count check, which would make this drop the last one.
4. A retained value is a deliberate leak. The container does not report it
   separately; the failure that caused it is reported where it is caught.

## Consequences

Destructors of retained values never run: a channel sender held by a retained
capture is never closed, and a retained render object keeps its resources until
process exit. This is the cost of not risking a second panic, and it is bounded
to operations that already failed. Healthy destruction is unchanged and keeps
container order.

The rule contains failures that reach a container's boundary; it cannot
contain one that never does. A single value whose own fields double-panic during
its destruction, or user code that double-panics, still aborts before control
returns to the container, as ADR-0104 states.

Tests that pin retention run in a child process, since the alternative they
guard against is an abort. They assert the first failure, the retained tail and
the next healthy operation on the same owner, and an aggregate whose fields
double-panic is not among the cases a container claims to survive.

## Migration

Containers that still drop user values during unwinding, or after a caught
failure in the same operation, disagree with this decision and are defects. For
example, `FocusManager::close` drops its taken callback vectors unconditionally.
They are brought into line by the retirement changes to focus, text input,
pointer routes, navigation, animation, rendering, notifiers and presentation
close that cite this ADR.

## Hit-test metadata retirement

Hit-test entries detach their opaque metadata before destruction. Healthy
retirement releases each owner; an entry destroyed during unwind retains its
metadata, so a framework-owned path cannot start a second payload destructor
after the first one fails. This also applies when a pending hover is replaced
before frame delivery.

Frame delivery has a separate caught-failure boundary: the binding retires
each metadata owner with its existing first failure before resuming that
failure, and continues delivering accepted frame peers. Publishing a newer
pending movement precedes outgoing retirement, so reentrant replacement and
subsequent healthy input remain deliverable. The public
`binding_input_contract_matrix` covers healthy destruction, callback and
metadata failure competition, queued replacement, source-metadata boundaries,
reentrant callbacks and recovery. Competing ordinary metadata destructors run
in child processes. A single opaque payload whose own fields double-panic
remains outside the containment guarantee.
