# ADR-0123: Exceptional presentation close withdraws authority before retaining ownership

- **Status:** Accepted
- **Date:** 2026-10-05
- **Supersedes:** [ADR-0048](ADR-0048-frame-transaction-boundary.md), only its
  remaining-element disposal policy when terminal presentation close already
  holds a failure. Ordinary per-element removal and frame recovery remain unchanged.

## Context

Closing one presentation must revoke its capabilities even when a lifecycle,
platform or retirement callback panics. Catching that panic ends the ambient
unwind, so a subsequent opaque destructor can compete with the saved failure.
Retaining a tree without withdrawing its keys and executable handles would
leave a closed presentation reachable through a sibling's shared realm.

## Decision

The host carries the first failure explicitly through terminal close. An active
outer unwind also selects preserving mode. Authority withdrawal commits before
optional user notifications: presentation input targets and cached routes,
liveness, graph creation and writes, external build inbox and rebuild handles,
realm and local GlobalKeys, agent ports, focus, text input, gestures and mouse
tracking become unavailable for the closed owner. Shared sibling capabilities
remain usable. Closed graph operations report `SignalError::OwnerClosed`.

Required platform cleanup, including IME disable and accessibility withdrawal,
is attempted after failure. Window and accessibility bridge leases remain
outside callback catches until their retirement. Arbitrary key owners are moved
out while guarded and retired after releasing all guards.

Once a failure is held, optional widget disposal and opaque ownership retirement
are suppressed; the removed presentation envelope is retained after authority
withdrawal. Later opaque values and panic payloads cannot replace the first
failure. Healthy close destroys its owners normally. Healthy closed-owner
rejection remains ordinary; an unrelated rejection unwind does not permanently
change that owner's terminal policy.

An ordinary `LifecycleSource::drain` still attempts all eligible callbacks,
including reentrant terminal callbacks, before resuming its first local panic.
The host suppresses subsequent optional terminal rounds only after that failure
returns to it, or when it already holds a failure. Required lifecycle commits
and the closing ladder still finish.

## Verification and limits

The production `realm_and_presentation_isolation_matrix` includes
`presentation_close_retirement_failures_preserve_focus_ime_and_siblings`:
bounded child cases exercise competing failures, active unwind, platform-owner
release, callback reentry, saved mixed-owner routes, key reuse and a sibling's
next real frame. Test execution and restored-defect controls are reported
separately from this architectural decision.

This policy cannot rescue the first opaque aggregate whose destructors already
double-panic before reaching a catch boundary. Ordinary generated envelope
destruction, standalone pipeline, post-frame and writer aggregates, and unrelated
realm fields require their own ownership guarantees. It does not establish
universal destructor containment or change the render-demand retry policy.
