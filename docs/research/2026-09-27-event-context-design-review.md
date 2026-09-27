# Event-context catalog: design challenge

This review separates callback-signature coverage from the viability of the
ownership contract. A green package test suite is not evidence that arbitrary
cross-presentation operations, overload, cancellation or panic recovery compose.

## Intended contract

- A callback receives a borrowed capability for one presentation's reactive
  graph. It is not a physical-event identity, transaction, propagation token or
  async lifetime.
- Composition forwards that context. Adapters acquire a source only at an
  existing owner boundary; no process-global writer or implicit current realm.
- Owner-bound imperative operations validate the caller before native state
  mutation. A signal's later `ForeignGraph` error cannot roll that mutation back.
- Signal reads during an event observe current state without adding build
  subscriptions. Capturing a previous build's value is not an equivalent read.
- User effects discovered during layout/build are deferred. Synchronous input
  callbacks remain synchronous where they already have a legal write phase.

## Attacks and decisions

| Scenario | Finding | Decision |
|---|---|---|
| A toolbar in presentation B resets a form in A | Field values change before A's callbacks reject B's graph: mixed success | Reject foreign contexts before mutation; preserve caller provenance, return a typed error |
| A saved writer calls a form operation during build | Matching graph alone does not make the phase writable | Preflight phase as well as ownership |
| A second event arrives before rebuild | A captured enabled/value flag may be stale; write-only context cannot inspect current state | Expose pure `ReadGraph` through the borrowed writer/context, not subscription or graph mutation |
| A queued assistive activation outlives callback replacement/removal | Capturing the old closure invokes obsolete behavior | Snapshot the event, resolve the live handler at delivery, cancel after disposal |
| An earlier post-frame callback panics | A drained local vector drops uninvoked terminal notifications | Preserve the uninvoked tail for a later frame; consume the offender, propagate panic, keep FIFO and shared/local provenance |
| User callbacks panic during form reset or messenger advancement | Temporary flags can remain stuck | Restore bookkeeping with guards; do not promise rollback of arbitrary user effects |
| Many animation ticks accumulate before a build | A finite batch still costs O(N), not bounded latency | Do not claim a bounded queue; a future progress-coalescing contract must distinguish terminal events from replaceable progress |
| A widget is disposed before deferred delivery | Suppressing invocation does not immediately release all captures | Mount checks prevent callbacks; queue lifetime still controls capture retention |

The cross-presentation form tests failed against the unguarded implementation:
reset produced `"initial"` instead of preserving `"edited"`, and edit produced
`"foreign edit"` instead of preserving `"initial"`. These are state assertions,
not duplicated implementations of the intended predicate.

The scheduler's mixed-lane test also failed before the fix: the next frame ran
only the newly registered callback `[3]`, not the preserved order `[1, 2, 3]`.
After the fixes, `cargo nextest run -p flui-scheduler -p flui-view --lib --locked
--no-fail-fast` passed 822 tests, and `cargo nextest run -p flui-widgets --test
widgets_it -E 'test(form::)' --locked --no-fail-fast` passed 26 form tests. These
targeted results do not replace the affected-package gate or platform CI.

Panic recovery is a contract for callers that catch and resume scheduler work.
The current native Windows boundary aborts after an escaping panic; this review
does not claim a surviving native Windows UI loses a dismissal notification.

## Alternatives, not just patches

Complexity assessment from the implementation, not a benchmark: form preflight
adds a linear pass over the existing field snapshot, not a scan of every form
or presentation. The scheduler already sorts its post-frame batch by ID; tail
restoration adds linear work only on failure. Neither improvement establishes a
frame-time budget. No new executor, dependency, global registry or per-node
frame-path lock is introduced.

**Silently switch a foreign form operation to the target's writer:** rejected.
It changes the explicit caller-context contract and hides cross-presentation
coordination. A future cross-window command should name its target and define
delivery/cancellation; local `reset(cx)` should not invent that protocol.

**Invoke animation effects directly during build:** rejected. It permits native
state changes before the signal guard refuses writes. Deferral is necessary for
the existing build-observed animation bridge, but does not eliminate that bridge.

**Give all animation listeners owner-local event contexts now:** potentially a
better end state, but changes shared controller ownership, not only widget
setters. It needs a separate migration of listener storage and dispatch phases.
The present bridge must not be advertised as the final animation architecture.

**Continue all callbacks after one panics:** rejected. FLUI's panic policy
propagates the operation's panic; retaining uninvoked work is recovery of queue
ownership, not per-callback exception isolation.

## External reference check

[Masonry 0.4's pass model](https://docs.rs/masonry/0.4.0/masonry/doc/internals_01_pass_system/index.html)
separates event, rewrite and rendering passes, with bounded rewrite reruns and
owner-targeted deferred mutations cancelled when their widget disappears.
Its deferred mutation facility is explicitly an escape hatch, not the default
widget update path. This supports challenging pervasive deferral rather than
copying an API name: FLUI's graph capability does not provide Masonry's mutable
tree access or event bubbling semantics.

[Flutter post-frame callbacks](https://api.flutter.dev/flutter/scheduler/SchedulerBinding/addPostFrameCallback.html)
run after the rendering pipeline in registration order and do not themselves
request a frame. Reusing that phase does not establish bounded delivery latency
or FLUI's Rust unwind policy.

## Publication boundaries still open

Shared PageView listeners, drag-target metadata, raw semantics handlers and
unmounted local-history callbacks have different ownership contracts. They must
not receive fabricated writers merely to make their signatures look uniform.
Direct `Reactive` write access and raw lifecycle/post-frame context migration
also remain named work in ADR-0086.

Form handles describe one mounted owner. Simultaneously mounting the same handle
twice is an existing unsupported case: shared value/controller/callback storage
is configured before lifecycle binding, and can be overwritten by the second
mount. The new checks do not turn that storage into a multi-owner handle. A
future exclusive-binding design needs an attachment/lease contract and a lawful
failure path in lifecycle, not a new caller-triggered panic disguised as an
internal invariant. Do not advertise the present checks as universal prevention
of handle aliasing.

Before expanding `EventCx`, require a concrete consumer and a propagation design.
Adding event identity, transactional semantics or async cancellation to a context
that is independently opened by several bridges would not make those guarantees
true. Presentation-local signals likewise are not a shared application model.
