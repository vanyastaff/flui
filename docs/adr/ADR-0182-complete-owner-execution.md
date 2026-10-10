# ADR-0182: The UI owner executes complete scheduler turns

- **Status:** Accepted
- **Date:** 2026-10-10
- **Supersedes:** ADR-0136's scheduler frame-entry API and ADR-0083's raw background execution sequence; ADR-0175's statement that scheduler frame entry points accept an owner argument.
- **Related:** [ADR-0083](ADR-0083-one-frame-transaction-in-flui-runtime.md), [ADR-0127](ADR-0127-exceptional-path-retention.md), [ADR-0175](ADR-0175-owner-local-scheduling-and-animation.md), [ADR-0178](ADR-0178-notification-first-failure-custody.md).

## Decision

`UpdateScheduler` is the producer and observation handle. Only `OwnerFrame`
executes a complete frame or background turn. Its private admission permit is
acquired before diagnostics, demand consumption or runtime preparation, and
remains active through completion delivery, callable retirement and temporary
scheduler release. Recursive, retired and closed execution return typed
refusals before invoking the submitted work. Retirement is permanent for that
owner; a replacement owns fresh task and post-frame storage.

A frame accepts an Idle preparation callable and a persistent pipeline callable.
The owner invokes both by borrowing their owned envelopes, once each. The
runtime prepares timestamp, owner commands, geometry and text-commit exclusion
inside admission, before transient callbacks. A background turn consumes old
demand, prepares geometry, then polls one ready batch without visual phases.
Failed preparation skips polling and preserves ready work; bounded existing
wake recovery repairs delivery debt when the host survives containment.

The owner holds a produced frame result outside later invocation catches.
Cleanup retains it after a failure, instead of running arbitrary result
destructors while unwinding. Preparation and pipeline callable envelopes retire
separately; dropping a tuple of two hostile envelopes could abort before the
boundary sees a failure. This does not extend borrowed invocation to consumed
`FnOnce` transient or post-frame callbacks.
The same custody covers refused envelopes, completion wakers, diagnostics,
recovery hooks and owner retirement. A live owner-local failure signal protects
reentrant cleanup even when user code has caught a nested panic and
`thread::panicking()` is false. It carries no payload and grants no execution
authority; the enclosing boundary retains the first payload. During admitted
execution, producer-side callback removal, hook replacement and task retirement
consult that same live signal before releasing an opaque envelope. The binding
is weak and scoped to the turn, so it neither extends owner lifetime nor changes
healthy destruction after the turn ends.

Temporary scheduler release distinguishes an ordinary reference release from
the actual last strong release. Actual destruction closes weak upgrades before
completion delivery and capture retirement, and detaches every opaque queue
before invoking user code. A terminal-only result sink passes a first teardown
failure back to the enclosing owner boundary. Keeping an upgraded scheduler
alive as a recovery shortcut would keep weak authority usable and is rejected.

`UiRuntime::pump` retains the frame transaction. Rejection creates no timestamp
or commit guard and runs no deferred-grant anchor. Admitted framework guards
restore enclosing state on return or unwind. Its internal result is a boolean,
so deferred native-grant failures after scheduler return cannot destroy an
opaque pipeline result. `HeadlessBinding` uses the same owner execution seam.

Physical pacing remains in presentation clocks and host drivers. Unsupported
performance-mode requests have no executor policy consumer and are removed;
the scheduler does not claim to tune a runtime by storing a request counter.

## Consequences

Hosts migrate raw phase driving to complete owner operations; producer handles
cannot independently consume demand or poll owner tasks. Preparation and
pipeline are fixed semantic slots, not caller-maintained phase tokens.
Retirement can withdraw future admissions during a synchronous callback, but
cannot preempt that callback or roll back application side effects.

ADR-0127's exceptional retention is deliberate leakage. Containment cannot
rescue opaque aggregate destruction that double-panics internally, user code
that double-panics, or `panic=abort`. Native callback survival remains the
host's contract. Cross-target typechecking does not establish live native
recovery behavior.

## Verification

`owner_callback_contract` observes nested frame/background refusal, permanent
retirement, produced-result custody and live completion-envelope custody after
a nested caught failure. `admitted_frame_preparation_contract` and
`owner_background_turn_contract` cover admitted preparation and wake recovery.
`owner_retirement_custody_contract` covers finished and cancelled futures,
callback and lifecycle-listener removal, hook replacement, eager completion,
and nested frame/background refusal from transient callbacks, task polls and
post-frame callbacks. The runtime
`frame_pacing_and_pump_matrix` observes text-store deferred grants and animation
time through rejected native-style reentry. These are behavioral contracts;
passing compilation alone does not establish their implementation.
